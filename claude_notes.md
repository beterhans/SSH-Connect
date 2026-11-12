Your handoff note is solid, and the Rust changes look right. My suggestions are below, roughly in priority order.

## 1. Unblock the test rig
- **Apply the one-line fix.** Add `t.load_server_moduli()` before `start_server`, as your note says.
- **Check the return value.** It returns `True` or `False`. If it's `False`, fail loudly instead of continuing.
- **Run the fallback separately.** Test `diffie-hellman-group14-sha1` on its own. If it passes, ssh-dss, aes128-cbc and hmac-sha1 are proven, and only gex is left to isolate.
- **Check the modulus size.** `/etc/ssh/moduli` on macOS may not contain moduli small enough for a legacy client's GEX request. Old clients ask for 1024–2048 bits. russh's `DH_GEX_SHA1` min/preferred/max sizes must overlap what the server pack holds. If the server finds no matching group, the handshake fails with an error that looks like a negotiation failure.
- **Fix the rig's own paramiko pin.** The rig's paramiko 3.5.1 is not your real device's stack. Your real script uses paramiko 2.7.2.

## 2. Add a negative control
Your note lists this as optional. I'd make it mandatory:
- Run the same test against the `*-01.old` build and confirm it fails on gex, dss and hmac-sha1.
- Without it, an all-PASS result could just mean the rig is too permissive.
- Also run the server in several single-algorithm modes. For example, run it with only ssh-dss and with only hmac-sha1. That shows which individual algorithm each failure comes from.

## 3. Keep legacy algorithms opt-in
Right now the legacy list is applied to every `ssh_connect`. That has two downsides:
- It weakens negotiation for modern hosts. The order is modern-first, so a modern server still picks a modern algorithm. But a MITM can strip the modern entries from the server's offer, and the client will then accept the legacy ones.
- A downgrade-style failure becomes silent.

Options:
- **Add a per-host flag in `hosts.example.toml`**, such as `legacy_ssh = true` (or `ssh_profile = "legacy"`). The default stays modern-only.
- **Add an auto-retry.** Try modern first. On a "no common algorithm" error, retry once with the legacy profile and log a warning that says so.
- **Add an optional `ssh_connect` argument** like `legacy: bool`, so the AI agent can choose it explicitly.

I'd go with the per-host flag plus the auto-retry. It's the safest and keeps the tool convenient for iSAM work.

## 4. Real-device risk: the broken DSA signature
This is the biggest unknown. Your Python script patches `DSSKey.verify_ssh_sig` to return `True`. That suggests some iSAMs send DSA signatures paramiko can't verify. russh has no bypass hook, so a real device may fail with a kex signature error.

If that happens, here are the options:
- **Confirm it first.** Run `ssh -vvv -oHostKeyAlgorithms=+ssh-dss -oKexAlgorithms=+diffie-hellman-group14-sha1 -oCiphers=+aes128-cbc -oMACs=+hmac-sha1 user@olt` against the real device. If OpenSSH (if it still supports ssh-dss) fails the same way, the device is the problem. If OpenSSH works, the problem is in russh.
- **Vendor russh and add a bypass.** Use a `[patch.crates-io]` fork with a feature flag that skips host-key signature verification. Only enable it per host, and log a warning, because it removes host authentication.
- **Fall back to a subprocess.** Shell out to a pinned legacy tool such as a bundled `ssh` binary or the Python/paramiko 2.7.2 script, for hosts marked `legacy_unverified`.
- **Reduce the risk instead of removing it.** Pin the host key fingerprint in `hosts.toml` (TOFU). A pinned key would restore a degree of host authentication even if the signature check is bypassed. Your `check_server_key` returning `true` currently accepts any key.

## 5. Security hygiene
- **Pin host keys.** Replace the blanket `check_server_key → true` with fingerprint pinning or `known_hosts` for at least the exec/SFTP path.
- **Log the negotiated algorithms.** Add something like `info!(kex, key, cipher, mac)` after connecting, so legacy use is visible. Consider surfacing it in the `ssh_connect` tool result as well.
- **Keep arcfour out of scope**, as you have it.
- **Mention the legacy mode in the README.** Add a short note on what it enables and why.

## 6. Keeping the Python script alive
Your pinned versions are `netmiko==2.4.2`, `paramiko==2.7.2`, `configparser==5.2.0`, `pyinstaller==4.10`, `textfsm<1.1` and `six==1.17.0` . A few notes:
- **Pin everything, including transitive dependencies.** Run `pip freeze` into a lock file. In particular, paramiko 2.7.2 needs an old `cryptography` and `bcrypt`/`pynacl`. A newer `cryptography` (which dropped DSA/SHA-1 pieces) can break it silently.
- **Rebuild the venv.** Your note says the `.venv` is broken or empty. Recreate it with `uv venv --python 3.8` (or 3.9) and `uv pip install -r requirements.txt`.
- **Containerize or ship the binary.** The PyInstaller build bakes in the old stack, so it is the safest delivery form. Rebuild it on the target OS (Fedora 28 or RHEL 8.6 as your header says).
- **Use the script as the reference.** Once the Rust path exists, run both against the same OLT and compare the output of `show equipment temperature`. That's your acceptance test.

## 7. Smaller code points
- Make the `legacy_compatible_preferred()` doc comment name the exact algorithms and why each is there.
- Add a unit test for `ssh_tools.rs` that checks it uses the shared preferences, so nobody later reverts it to `Config::default()`.
- The iSAM script sets `stty erase ^H` because the iSAM CLI uses it. If your MCP console path sends backspaces, make sure it sends `^H` for those hosts. Otherwise edited commands will be garbled.
- The iSAM prompt regexes the script waits on (`[>#\$]`) show the CLI may not return a normal shell prompt. Make sure `ssh_exec` works on iSAM, because many OLT CLIs reject exec channels and only support an interactive shell. If so, the console path (shell channel) is the real target for these devices.

## Suggested next steps
1. Apply `load_server_moduli()` and re-run the test rig.
2. Run the negative control.
3. Test against the real iSAM with OpenSSH `-vvv` first, then the Rust tool.
4. Add the per-host `legacy_ssh` flag with auto-retry.
5. Pin host keys.

I can draft any of these, such as the `hosts.toml` flag with the retry logic in `ssh_tools.rs`, or the fixed `legacy_ssh_server.py` handler. For code-level help, paste `transport/ssh.rs` and `ssh_tools.rs`, since I can't see your repo.

This is an AI generated response, it may not be fully accurate. Please verify the information independently.

