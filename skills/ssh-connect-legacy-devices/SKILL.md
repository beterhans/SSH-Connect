---
name: ssh-connect-legacy-devices
description: Reach old network equipment that only speaks legacy SSH — Alcatel-Lucent/NTT iSAM OLTs, 2015-era Cisco IOS-XE, old appliances — over the ssh-connect MCP server. Covers SHA-1/DSA algorithm negotiation, device prompt regexes, the ^H erase character, exec-vs-shell channel differences, pager handling and how to verify a legacy handshake. Use when a connection fails with "No common Kex/Key/Mac algorithm" or "Wrong server signature", or when working with an OLT or switch whose CLI is not a normal shell.
---

# Legacy devices (iSAM OLTs, old IOS-XE, old appliances)

## 1. Which algorithms are negotiated

`russh` is configured with a modern-first list plus legacy fallbacks, so old
gear connects without weakening modern hosts:

| Category | Modern (preferred) | Legacy fallback |
|---|---|---|
| kex | curve25519, DH-G16/G14-SHA256 | **group-exchange-sha1, group14-sha1, group1-sha1** |
| host key | ed25519, ECDSA, RSA-SHA2 | **ssh-rsa (SHA-1), ssh-dss (DSA)** |
| cipher | chacha20-poly1305, aes-gcm, aes-ctr | **aes-cbc, 3des-cbc** |
| MAC | hmac-sha2-512/256 | **hmac-sha1** |

`arcfour`/RC4 is not implemented in russh — irrelevant for anything post-2010.

Verify what was actually negotiated with `RUST_LOG=debug`; the server logs:

```
DEBUG russh::client::kex: negotiated algorithms: Names { kex: Name("diffie-hellman-group-exchange-sha1"), key: Dsa, cipher: Name("aes128-cbc"), client_mac: Name("hmac-sha1"), … }
```

## 2. Use the console path, not `ssh_exec`

OLT and switch CLIs are **not** a shell. They:

* reject exec channels (only an interactive PTY shell works),
* keep state across commands (`configure …` → `exit all`),
* prompt for a username/password *inside* the session (`login` tool),
* page their output (`--More--`).

So: `connect` → `run_command` → `disconnect`. Use `ssh_exec` only for a real
Linux/BSD shell.

## 3. Prompt regex — the #1 cause of "it times out" or "wrong output"

`run_command` ends when the device's prompt matches the regex. Two failure
modes, both caused by the regex:

**Not matching** → the call burns the whole timeout (30 s) and returns
`timed_out: true` with partial output.

**Matching too early** → the command's output is attributed to the *next*
command, or comes back empty. This happens when the regex is **not
end-anchored**: a device that redraws its prompt while accepting the previous
line (zsh does, and so do several switch CLIs) puts the prompt text and the
echoed command on the *same line*, so a regex that merely *contains* the prompt
matches that line too.

**Always anchor with `\s*$`** and keep the default's style (restricted character
class, no spaces):

| Device | Regex |
|---|---|
| default (most switches) | `[\w.@:~()/\\-]+[#>$]\s*$` |
| iSAM / typical CLI | `[#>$]\s*$` |
| zsh (user@host path %) | `[\w.-]+@[\w.-]+ [^\r\n]*%\s*$` |

Set it per host in `hosts.toml` (`prompt = "..."`) or per call
(`connect {prompt: "..."}`).

Check `matched_prompt` and `timed_out` in every result: `timed_out: true` means
the regex did not match; output that looks like the *previous* command means it
matched too early.

The transcript log shows the exact prompt bytes — match against that, not a
guess.

## 4. The `^H` erase character

Many old CLIs (the iSAM included) use `^H` (backspace, `0x08`) as the erase
key — the Python/Netmiko script does `stty erase ^H` before connecting.

The tool applies backspaces with their **real erase semantics**, so a
`<char>^H<char>` redraw is handled correctly. Before that fix such devices
showed a duplicated first character (`uuname -a` instead of `uname -a`).

A pager's `^H ^H` erase-in-place is still dropped, so `--More--` survives for
the pager handling.

## 5. Pagers and multi-line output

`--More--` is auto-advanced (space) and the pages are stitched together, up to
4096 pages. If the device instead uses a different pager prompt, send
`terminal length 0` / `screen-length 0` first to disable paging entirely.

## 6. Device-level login / enable

Two different layers — do not mix them up:

* **SSH-layer auth** (username/password/key) → the `connect` arguments
  (`username`, `password`, `key_path`). The device's own CLI login is separate.
* **CLI-layer prompts** → `login {session, username, password}` for a console
  that asks `login:` / `Password:`, and `enable {session, password}` for
  privileged mode. `expect_send {session, expect, send}` answers one-off
  prompts such as `[confirm]`.

Secrets sent through `login`/`enable` are never echoed back.

## 7. Diagnosing a failed legacy handshake

| Error | Meaning | Fix |
|---|---|---|
| `No common Kex algorithm` | device offers no kex we support | add its kex to the preference list |
| `No common Key algorithm` | host-key type not offered | add it (e.g. `Algorithm::Dsa`) |
| `No common Mac algorithm` | MAC not offered | add it (e.g. `hmac-sha1`) |
| `Wrong server signature` + log `unsupported algorithm: ssh-dss` | russh's `dsa` feature is off | enable russh's `dsa` feature in `Cargo.toml` |
| `Wrong server signature` (no "unsupported") | device sends a broken DSA signature | see §8 |

Always read the server's **stderr** — `RUST_LOG=debug` shows the negotiated
algorithms and the underlying error. The tool's own error text can be generic
("Wrong server signature") while the log names the real cause.

## 8. Broken DSA signatures — the one real unknown

Some iSAMs send DSA signatures that fail verification. russh verifies
properly and has **no bypass hook**, so such a device cannot be connected
without one of these:

1. **Confirm first** with OpenSSH on a Linux box (OpenSSH 8.x still supports
   `ssh-dss`; OpenSSH 10.x removed it):
   ```sh
   ssh -vvv -oHostKeyAlgorithms=+ssh-dss \
          -oKexAlgorithms=+diffie-hellman-group14-sha1 \
          -oCiphers=+aes128-cbc -oMACs=+hmac-sha1 user@<device>
   ```
   If OpenSSH fails the same way → the device is at fault. If OpenSSH works →
   the bug is in russh.
2. **Pin the host key** (fingerprint in `hosts.toml` / `known_hosts`) — restores
   authentication even if the signature check is weakened.
3. **Fall back** to the Paramiko/Netmiko script for that device only.

## 9. Verifying without the device

A fake legacy SSH server (Paramiko, offering *only* old algorithms) proves the
whole path end to end. Recipe and the full test matrix live in the project:
`LEGACY_SSH_TESTING.md`.

```sh
cd "$HOME/Downloads/temp"
./.venv-legacytest/bin/python mcp_legacy_test.py   # end-to-end, both tool paths
./.venv-legacytest/bin/python matrix_test.py        # per-algorithm isolation
```

A PASS means the client offers every algorithm the (legacy-only) server
demands; a FAIL names the exact gap.
