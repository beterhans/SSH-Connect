# Handoff: legacy SSH algorithm support in ssh-connect MCP tool

**Date:** 2026-11-12 · **Status: code changes DONE + unit-tested; one E2E test-rig bug blocks full verification**

## 1. Task

Make the Rust MCP tool (`/Users/hans/Documents/source_code/MCP/SSH-Connect`) able to
connect to old network devices (Alcatel-Lucent iSAM OLTs) that only support
"less secure" SSH — matching what the user's Python script can do:

`/Users/hans/Documents/source_code/PycharmProjects/iSAM_line_Check/iSAM_line_Check.py`
(lines 67–79: forces old kex via Netmiko/Paramiko, plus a `DSSKey.verify_ssh_sig →
True` monkey-patch → the devices present **DSA (`ssh-dss`) host keys** and/or
SHA-1 key exchange).

## 2. Gap analysis (verified in russh 0.60.3 source, `~/.cargo/registry/src/*/russh-0.60.3`)

| capability | Python script (paramiko 2.7.2) | Rust tool BEFORE fix |
|---|---|---|
| kex g14-sha1 / g1-sha1 | ✓ forced | ✓ console path only (`transport/ssh.rs`) |
| kex **group-exchange-sha1** (GEX) | ✓ forced | ✗ missing — russh DOES support it (`kex::DH_GEX_SHA1`, registered, no feature gate) |
| host key **ssh-dss (DSA)** | ✓ (with verify-bypass hack) | ✗ missing — `Algorithm::Dsa` exists in russh's forked ssh-key and verifies properly |
| host key ssh-rsa (SHA-1) | ✓ | ✓ console path only |
| CBC/3DES ciphers | ✓ | ✓ console path only (`des` feature enabled in Cargo.toml) |
| MAC **hmac-sha1 / -etm** | ✓ (paramiko defaults) | ✗ — russh 0.60 `Preferred::DEFAULT` MAC list is SHA-2-only (explicitly excludes SHA-1); both implemented & registered in `mac/mod.rs`, no feature gate |
| arcfour (RC4) | paramiko can, not default | ✗ not implemented in russh at all (pre-2010 only — out of scope) |
| server-ops path (`ssh_connect`/`ssh_exec`/SFTP tools) | n/a | ✗ used `client::Config::default()` = modern-only, zero legacy |

The comment in old `transport/ssh.rs` line 65 ("Default MAC order already ends with
hmac-sha1") was **stale/wrong** for russh 0.60 and replaced.

## 3. Changes made (DONE, build + `cargo test` 25/25 pass)

Backups exist: `src/transport/ssh.rs-01.old`, `src/tools/ssh_tools.rs-01.old`
(user rule: comment out, don't delete — honored for the replaced line in ssh_tools.rs).

### 3.1 `src/transport/ssh.rs`
- `legacy_compatible_preferred()` → made **pub** (shared by both paths).
- kex list: added `kex::DH_GEX_SHA1` (first among the SHA-1 legacy group).
- key list: added `Algorithm::Dsa` (last = lowest priority).
- MAC: replaced stale comment with explicit `mac:` field — modern first, then
  `HMAC_SHA1_ETM`, `HMAC_SHA1`.
- Doc comment updated (iSAM/DSA/GEX mentioned).
- Unit test extended: asserts gex-sha1, ssh-dss, hmac-sha1 present + modern-first ordering.

### 3.2 `src/tools/ssh_tools.rs` (line ~144, in `ssh_connect`)
```rust
// let config = Arc::new(client::Config::default());   // old line kept as comment
let config = Arc::new(client::Config {
    preferred: crate::transport::ssh::legacy_compatible_preferred(),
    ..Default::default()
});
```
→ exec/SFTP tools now use the same legacy-compatible preferences as the console path.

### 3.3 Run note
`_agent/2026-11-12_1243.md` (current target / key points).

## 4. E2E test rig (in `$HOME/Downloads/temp`)

Goal: prove a real handshake succeeds against a server offering ONLY the legacy combo
`gex-sha1 + ssh-dss + aes128-cbc + hmac-sha1` (any missing client-side algo →
"no common algorithm" failure), over BOTH MCP tool paths.

Files:
- `legacy_ssh_server.py` — paramiko 3.5.1 server, legacy-only via the sanctioned
  `Transport(sock, disabled_algorithms={...})` kwarg (paramiko 3.x preference
  properties are READ-ONLY — do not use `t.preferred_kex = ...`).
  Forces: kex=gex-sha1 only, host key=ssh-dss (DSA-1024 generated at startup),
  cipher=aes128-cbc only, mac=hmac-sha1 only. Password auth `admin`/`testpass`,
  port arg (default 22443). Echoes `echo …` commands, handles shell + exec channels.
- `mcp_legacy_test.py` — spawns the server + `target/debug/ssh-connect`, speaks
  MCP JSON-RPC over stdio (NDJSON): `initialize` → console `connect` (+wake) →
  `ssh_connect` + `ssh_exec "echo hello-legacy"` → disconnects. Prints PASS/FAIL per step,
  exit code 0 = all pass. MCP server stderr → `mcp_stderr.log` (run with RUST_LOG=debug).
- `.venv-legacytest/` — uv venv with **paramiko==3.5.1** (NOTE: paramiko 5.x removed
  DSA/`DSSKey`; the user's real script pins paramiko==2.7.2 + netmiko 2.4.2).

Run:
```bash
cd "$HOME/Downloads/temp" && ./.venv-legacytest/bin/python mcp_legacy_test.py
```

## 5. CURRENT BLOCKER (in the test rig, NOT in the Rust code)

Symptom: server log `Incompatible ssh peer (no acceptable kex algorithm)` even though
the client's KEXINIT visibly contains `diffie-hellman-group-exchange-sha1` and the
server's KEXINIT also advertises it.

Root cause (paramiko 3.x, `transport.py`):
- Server-side group-exchange needs a **modulus pack** (pre-computed safe primes).
  `Transport._modulus_pack` starts as `None`.
- In `_send_kex_init()` (line ~2418): if `self._modulus_pack is None` and any
  gex algo is enabled, paramiko strips ALL `diffie-hellman-group-exchange-sha*`
  from its own negotiated list via `self.get_security_options().kex = pkex`, which
  **assigns an instance attribute `_preferred_kex`** (now empty). The KEXINIT already
  went out advertising gex-sha1, but the subsequent `_parse_kex_init()` negotiates
  against the now-empty list → "no acceptable kex algorithm".

Fix (one line in `legacy_ssh_server.py` `handle()`, before `start_server`):
```python
t.load_server_moduli()   # auto-loads /etc/ssh/moduli — EXISTS on this Mac (766 KB,
                         # verified 2026-11-12). Returns True on success.
```
Method name is `load_server_moduli` (NOT `load_moduli`). It checks `/etc/ssh/moduli`,
then `/usr/local/etc/moduli`; optional explicit filename arg.

After that: re-run `mcp_legacy_test.py`. Expected all-PASS → done (both paths verified).

Fallback if moduli loading misbehaves: change the server's `keep["kex"]` to
`"diffie-hellman-group14-sha1"` (fixed 2048-bit group, no moduli needed) — still
exercises ssh-dss + aes128-cbc + hmac-sha1; then re-test gex separately.

## 6. Remaining verification after the rig is fixed

1. `mcp_legacy_test.py` → all steps PASS (console connect + exec round-trip).
2. Optional: negative control — run the test against a build WITHOUT the fix
   (stash changes / use `ssh.rs-01.old` + old ssh_tools) to confirm it FAILS on
   gex/dss/hmac-sha1 (proves the test actually detects the gap).
3. Optional: user's real iSAM device if reachable from this machine — that's the
   ultimate acceptance test.

## 7. Risk note (carry over to real-device testing)

The Python script's `DSSKey.verify_ssh_sig → True` hack suggests some iSAMs may emit
**broken DSA signatures during kex**. russh verifies the host-key signature properly
and has **no bypass hook** (the console path's `check_server_key` returns true, but
kex-signature verification happens inside russh negotiation and would hard-fail).
If a real iSAM fails with a kex signature error, that's the cause — would need a
russh-level workaround (rare; most iSAMS work fine).

## 8. Environment facts worth keeping

- russh **0.60.3** (Cargo.lock). `Preferred::DEFAULT`: kex = MLKEM768X25519,
  curve25519(-pre-RFC), DH-GEX-SHA256, G18/17/16/15-SHA512, G14-SHA256 (+ext names);
  key = Ed25519, ECDSA p256/p384/p521, ssh-rsa(sha512/sha256/**sha-none**);
  cipher = chacha20, aes256-gcm, ctr-256/192/128; mac = SHA512/SHA256 ETM+plain
  ("SHA-1 MAC variants are excluded from defaults"); compression = none (+zlib w/ flate2).
  Full support: `DH_GEX_SHA1`, `Algorithm::Dsa` ("ssh-dss"), `mac::HMAC_SHA1(_ETM)`,
  aes*-cbc, 3des-cbc (`des` feature). NO arcfour.
- russh source: `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/russh-0.60.3`
  (pulled via `cargo fetch`). ssh-key fork:
  `internal-russh-forked-ssh-key-0.6.18+upstream-0.6.7` (same registry dir).
- Tool→path map: `connect`/`disconnect`/console tools → `tools/session_tools.rs`
  → `transport/ssh.rs::SshTransport`; `ssh_connect`/`ssh_exec`/`ssh_*_file` →
  `tools/ssh_tools.rs` (+ `state.rs`). Both now share the legacy preferences.
- iSAM script: paramiko==2.7.2, netmiko==2.4.2 (its `.venv` is broken/empty — no python binary).
