# How to test the legacy-SSH support

Everything below is **verified working** as of 2026-11-12.
Results: isolation matrix **5/5 PASS**, end-to-end **4/4 PASS**, negative control **0/5** (pre-fix build).

## 0. What was fixed (3 changes)

| # | File | Change |
|---|---|---|
| 1 | `src/transport/ssh.rs` | added `kex::DH_GEX_SHA1`, `Algorithm::Dsa` (ssh-dss), explicit MAC list with `hmac-sha1`; made the fn `pub` |
| 2 | `src/tools/ssh_tools.rs` | `ssh_connect` now uses the shared legacy preferences instead of `Config::default()` (modern-only) |
| 3 | `Cargo.toml` | **`features = [..., "dsa"]`** ← the critical one |

Change 3 was the real blocker: without russh's `dsa` feature, `ssh-dss` was
*advertised* but unverifiable → handshake died with
`Wrong server signature` (underlying error: `unsupported algorithm: ssh-dss`).

## 1. Fastest check (30 seconds)

```bash
cd /Users/hans/Documents/source_code/MCP/SSH-Connect
cargo build && cargo test
```
Expect: `test result: ok. 25 passed`.

The unit test `preferred_includes_legacy_algorithms_after_modern_ones` asserts
gex-sha1 / ssh-dss / hmac-sha1 are all present and that modern algos stay first.

## 2. Real handshake test — no device needed

A fake legacy SSH server (paramiko) that offers **only** old algorithms, plus an
MCP client that drives the real binary over stdio JSON-RPC.

```bash
cd "$HOME/Downloads/temp"

# end-to-end: console path + exec path, with a command round-trip
./.venv-legacytest/bin/python mcp_legacy_test.py
```
Expected:
```
[PASS] mcp initialize
[PASS] console connect (gex-sha1 + ssh-dss + aes128-cbc + hmac-sha1)
[PASS] exec ssh_connect (same legacy profile)
[PASS] exec ssh_exec echo round-trip        # stdout: hello-legacy
=== 4/4 steps passed ===
```

## 3. Per-algorithm isolation matrix

```bash
./.venv-legacytest/bin/python matrix_test.py
```
Expected: **5/5 combos passed**. Each combo varies one algorithm so a failure
points at exactly one gap:

| combo | proves |
|---|---|
| gex-sha1 + ssh-dss + aes128-cbc + hmac-sha1 | the full legacy profile (your iSAM case) |
| g14-sha1 + ssh-dss + … | fixed-group kex works too |
| gex-sha1 + ssh-rsa + … | DSA is the only host-key gap |
| g14-sha1 + ssh-rsa + … | ssh-rsa (SHA-1) works |
| gex-sha1 + ssh-rsa + aes128-cbc + **hmac-sha2-256** | modern MAC still negotiates (no regression) |

To point the matrix at another build (e.g. the pre-fix binary):
```bash
SSH_CONNECT_BIN=/path/to/other/ssh-connect ./.venv-legacytest/bin/python matrix_test.py
```

## 4. Negative control — prove the test is real

```bash
cd /Users/hans/Documents/source_code/MCP/SSH-Connect
git worktree add "$HOME/Downloads/temp/prefix" HEAD
cd "$HOME/Downloads/temp/prefix" && cargo build
cd "$HOME/Downloads/temp"
./.venv-legacytest/bin/python mcp_legacy_test.py \
  --binary "$HOME/Downloads/temp/prefix/target/debug/ssh-connect"
```
Expected on the **pre-fix** build: **FAIL**, with these three distinct errors —
this is what each fix addressed:

- `No common Kex algorithm` → missing gex-sha1
- `No common Key algorithm` → missing ssh-dss
- `No common Mac algorithm` → missing hmac-sha1

Then clean up:
```bash
cd /Users/hans/Documents/source_code/MCP/SSH-Connect
git worktree remove "$HOME/Downloads/temp/prefix" --force
```

## 5. Against a real iSAM OLT

The rig proves the protocol side. The real device adds two CLI quirks — check
them after the handshake succeeds:

1. **Erase character.** The iSAM CLI uses `^H` (backspace) as its erase key.
   The Python script does `os.system("stty erase ^H")` before connecting. If
   edited commands come back garbled, the console path must send `^H`.
2. **Prompt.** The script waits on `[>#$]`, so the CLI may not emit a normal
   shell prompt — pass a custom `prompt` regex to `connect` if detection fails.
3. **Exec vs shell.** Many OLT CLIs reject exec channels and only support an
   interactive shell → use the `connect` (console) path, not `ssh_exec`.

Reference command (OpenSSH 10.3 on macOS **cannot** test ssh-dss — DSA was
removed; use a Linux box with OpenSSH ≤ 8.x for that):
```bash
ssh -vvv -oHostKeyAlgorithms=+ssh-dss \
    -oKexAlgorithms=+diffie-hellman-group14-sha1 \
    -oCiphers=+aes128-cbc -oMACs=+hmac-sha1 admin@<olt-ip>
```

## 6. Test files (all in `$HOME/Downloads/temp`)

| file | purpose |
|---|---|
| `legacy_ssh_server.py` | paramiko server offering ONLY legacy algorithms (`--kex/--hostkey/--cipher/--mac` flags) |
| `mcp_legacy_test.py` | drives the real binary over MCP stdio JSON-RPC (end-to-end) |
| `matrix_test.py` | per-algorithm isolation matrix |
| `dsa_math_check.py` | pure-Python DSA verify of a captured signature (used to prove paramiko's sig was valid) |
| `.venv-legacytest/` | uv venv with **paramiko==3.5.1** (5.x dropped DSA/`DSSKey`) |

Notes on the rig:
- paramiko 3.x preference properties are **read-only** → use
  `Transport(sock, disabled_algorithms={...})`.
- server-side GEX needs `t.load_server_moduli()` (`/etc/ssh/moduli` exists on
  this Mac) or paramiko silently drops gex from its own negotiation list.
- `--kex diffie-hellman-group14-sha1` is the fallback if moduli ever misbehave.

## 7. Debugging a failed connect

The MCP server logs to **stderr**. Run the binary with `RUST_LOG=debug` and read
the log — the negotiated algorithms and the real error appear there:
```
DEBUG russh::client::kex: negotiated algorithms: Names { kex: Name("diffie-hellman-group-exchange-sha1"), key: Dsa, cipher: Name("aes128-cbc"), ... }
DEBUG russh::client::kex: wrong server sig: signature::Error { source: Some(unsupported algorithm: ssh-dss) }
```
That "unsupported algorithm" line is exactly what the missing `dsa` feature
produced — check it first if DSA hosts fail again.
