# How SSH-Connect (this MCP server) works

Companion to `LEGACY_SSH_TESTING.md`. Read this to understand what the tool does,
how a session is created, where your credentials come from, and how host-key
trust is handled.

---

## 1. What it is

A single Rust binary that speaks the **Model Context Protocol (MCP)** over
**stdio**. It is a *server*: an AI client (Claude Code, opencode, …) starts it as
a subprocess and calls its tools.

```
┌──────────────┐   JSON-RPC over stdio    ┌───────────────────────┐
│  MCP client  │ ───────────────────────► │  ssh-connect (Rust)   │
│ (Claude Code │ ◄─────────────────────── │                       │
│  opencode …) │   tool results (stdout)  │  ┌─────────────────┐  │
└──────────────┘                          │  │ russh / serial  │  │
                                          │  │ telnet          │  │
                                          │  └─────────────────┘  │
                                          └───────────┬───────────┘
                                                      │ SSH / Telnet / serial
                                                      ▼
                                                  device / server
```

* **stdout = protocol only** (JSON-RPC). **stderr = logs** (`RUST_LOG=debug` for detail).
* Built on `rmcp` (MCP SDK). SSH is pure Rust via `russh` — no OpenSSL/C dependency.

## 2. Two interaction models, one process

| Goal | Tool family | Connect | Run with |
|---|---|---|---|
| Linux/VPS admin, needs exit codes | **server-ops** | `ssh_connect` | `ssh_exec`, `vps_*`, `ubuntu_*` |
| Switches / OLTs / consoles | **interactive console** | `connect` | `run_command`, `enable`, `login`, … |

### Server-ops path
`ssh_connect` → returns a `connectionId`. Every `ssh_exec` opens a **fresh SSH
channel**, runs one command, reads stdout/stderr + exit code, closes the channel.
Stateless per command, no PTY. File transfer uses SFTP on a side channel.

### Interactive console path
`connect` opens **one persistent channel with a PTY** and keeps it alive in a
session registry. `run_command` writes into that same shell and reads until a
prompt regex matches. This is the path for network-device CLIs, because they
are stateful (`configure …` → `exit all`, prompts change, pagers).

**Your iSAM OLT belongs to the console path.** OLT CLIs typically reject exec
channels, and their prompt is not a normal shell prompt.

## 3. Tool catalogue (37 tools)

**Session / discovery** — `connect`, `disconnect`, `list_sessions`, `list_com_ports`, `list_hosts`
**Interactive commands** — `run_command`, `run_commands`, `enable`, `login`, `expect_send`, `run_on_fleet`, `upload_config`, `download_config`
**Switch helpers** — `switch_backup_config`, `switch_network_diagnostics`
**Server-ops SSH** — `ssh_connect`, `ssh_exec`, `ssh_upload_file`, `ssh_download_file`, `ssh_list_files`, `ssh_disconnect`
**VPS/Ubuntu** — `vps_system_stats`, `vps_logs`, `vps_process`, `vps_file_read`, `vps_file_ops`, `vps_cron`, `ubuntu_service_control`, `vps_nginx_config`, `ubuntu_nginx_control`, `ubuntu_update_packages`, `ubuntu_ssl_certificate`, `ubuntu_website_deployment`, `ubuntu_ufw_firewall`, `ubuntu_mysql`, `vps_redis`, `vps_docker`, `vps_health_check`

`skills/` bundles operational playbooks (`switch-management`, `vps-management`,
`vps-health-report`, `health-report`); `.claude/agents/` has 5 sub-agents.

## 4. Where credentials come from

Precedence: **explicit tool argument → `hosts.toml` inventory → `~/.ssh/config`**.

* **`hosts.toml`** (gitignored; copy from `hosts.example.toml`; override path with
  `SSHCONNECT_HOSTS`) — per-host defaults so `connect {name: "olt-01"}` just works.
* **`~/.ssh/config`** — resolved by asking the *local* `ssh` itself: both connect
  tools run `ssh -G <host>` and use the HostName / User / Port / IdentityFile it
  reports. So `Host`, `Match`, `Include`, wildcards and percent tokens behave
  exactly as they do for `ssh` itself.
* **Environment variables** — for host `core-sw-01` the password override is
  `SSHCONNECT_CORE_SW_01_PASSWORD` (name upper-cased, non-alphanumerics → `_`).
  Same pattern for `USERNAME`, `HOST`, `PORT`, `KEY_PATH`.
* **ssh-agent** — identities from the agent are offered after any password and
  key file (`SSH_AUTH_SOCK`, or the Windows named pipe).

## 5. Host-key trust — how it actually works today

**Your understanding is correct.** A host key *is* the server's fingerprint
(identity). The desirable behaviour is exactly what you described: learn it on
first contact, then alarm if it ever changes.

### The mechanism exists — trust-on-first-use (TOFU)

`src/state.rs` implements it:

* Enabled with `SSHCONNECT_HOST_KEY_CHECK=tofu` (legacy alias `VPS_HOST_KEY_CHECK`;
  values `tofu|1|true|on|yes`).
* File: `~/.ssh-connect/known_hosts`, one line per host: `host:port <openssh-key>`.
* **First connection** → the key is *learned*: appended to the file, fingerprint
  logged, connection allowed.
* **Later connection, same key** → allowed.
* **Later connection, different key** → refused, and this is logged loudly:
  ```
  ERROR host key MISMATCH — refusing connection (possible MITM).
        Remove the stale line from ~/.ssh-connect/known_hosts if this change is expected.
  ```
  The connection is aborted, so the alarm is not silent.

### Two gaps you should know about

1. **It is OFF by default.** Without the env var, *any* key is accepted — a
   first-connection MITM would be accepted silently, and a later key *change*
   would also be accepted silently. Set `SSHCONNECT_HOST_KEY_CHECK=tofu` to get
   the behaviour you described.
2. **It only covers the server-ops path** (`ssh_connect` / `ssh_exec` / SFTP).
   The **console path** (`connect` — the one your iSAM needs) uses a different
   handler (`src/transport/ssh.rs`) whose `check_server_key` returns `Ok(true)`
   unconditionally — no learning, no mismatch alarm.

So today: TOFU exists, is off by default, and does not cover the console path.
The iSAM script has the same exposure (paramiko's `AutoAddPolicy`).

### Honest limits of TOFU

* TOFU can only detect a change **from the 2nd connection onward**. On the very
  first connection there is nothing to compare against — that is inherent to
  TOFU, not a bug.
* The alarm fires on **key change**. It cannot detect a MITM that proxies the
  *same* key (or a device that was already compromised before first contact).
* For a device you cannot re-key, the stronger option is pinning a specific
  fingerprint in `hosts.toml` — that restores authentication even if the
  signature check itself is weakened.

## 6. Legacy ("less secure") SSH support

Now supported for **both** paths — see `LEGACY_SSH_TESTING.md` for the test rig.

Negotiated, modern first, legacy only as fallback:

| Category | Modern | Legacy fallback |
|---|---|---|
| kex | curve25519, DH-G16/G14-SHA256 | **DH group-exchange-sha1**, group14-sha1, group1-sha1 |
| host key | ed25519, ECDSA, rsa-sha2 | **ssh-rsa (SHA-1)**, **ssh-dss (DSA)** |
| cipher | chacha20, aes-gcm, aes-ctr | aes-cbc, 3des-cbc |
| MAC | hmac-sha2-512/256 (ETM + plain) | **hmac-sha1** (ETM + plain) |

`arcfour`/RC4 is not implemented in russh at all — out of scope.

## 7. Broker — sharing live sessions between clients

The first instance to start becomes the **owner** of a per-machine rendezvous
endpoint (Windows named pipe `\\.\pipe\ssh-connect-broker-v1`; on Unix a socket
under `$XDG_RUNTIME_DIR`, else the temp dir). It holds the live connections and
COM ports. Later instances act as **proxies** forwarding `tools/call` to the
owner.

Consequence: **a session opened by one MCP client is usable from another** on
the same machine — that is how you can watch what an agent is doing to a device.

## 8. Session transcript log (every byte, printable)

The tool keeps a **per-session transcript** of every byte that crosses the wire,
in both directions, with direction markers and timestamps:

```
2026-11-12T11:06:32.373Z TX uname -a\n
2026-11-12T11:06:32.375Z RX u\x08unam
2026-11-12T11:06:32.375Z RX e -
2026-11-12T11:06:32.375Z RX a\e[?2004l\r\r\n
2026-11-12T11:06:32.379Z RX Darwin MacMini2018.local 24.6.0 Darwin Kernel Version 24.6.0 …
```

* Location: `~/.ssh-connect/logs/<session>.log` (`SSHCONNECT_SESSION_LOG_DIR` to move it).
* Disable: `SSHCONNECT_SESSION_LOG=off`.
* The path is returned by `connect` (field `transcript`) and by `list_sessions`.

Both knobs are plain environment variables, so they belong in the MCP server
config — in opencode that is `~/.config/opencode/opencode.json`:

```json
"ssh-connect": {
  "type": "local",
  "command": ["/path/to/ssh-connect"],
  "environment": {
    "SSHCONNECT_HOSTS": "/path/to/hosts.toml",
    "SSHCONNECT_SESSION_LOG_DIR": "/Users/you/.ssh-connect/logs"
  }
}
```

Set `"SSHCONNECT_SESSION_LOG": "off"` in the same block to disable transcripts
entirely (useful when you do not want secrets written to disk).
* Control characters are escaped (`\r`, `\n`, `\t`, `\e`, `\xNN`) so the file stays
  readable; nothing is masked — **secrets appear in the transcript**, so treat
  these files as sensitive.

This is the debugging counterpart to `run_command`'s output: the tool strips ANSI,
the echoed command and the trailing prompt, so the transcript is the only place
the *exact* bytes are preserved.

## 9. Watching the traffic (live visibility)

The server does **not** log individual commands by default. Three ways to see them:

1. **Read the tool results** — every `run_command` returns the exact command and
   the device output.
2. **The transcript log** (above) — the raw bytes, both directions.
3. **Drive the binary directly** (no MCP client needed) — a transcript of every
   tool call and result:
   ```bash
   cd "$HOME/Downloads/temp"
   SSH_PW='<password>' ./.venv-legacytest/bin/python mcp_drive.py \
     --host <IP> --user <user> --port 22 --command "show version"
   ```
   `--mode exec` uses the server-ops path instead; `--command` is repeatable;
   `--prompt '<regex>'` overrides the prompt detection; `--key-path` uses a
   private key; `--no-agent` stops agent identities being offered.
   With no password and no key it **prompts interactively** (the terminal
   equivalent of an MCP client's credential pop-up).

## 10. Practical notes for an iSAM OLT

* Use the **console path** (`connect` + `run_command`), not `ssh_exec`.
* **Erase character**: the iSAM CLI uses `^H` (backspace) as its erase key. The
  tool now applies backspaces with their real erase semantics, so a
  `<char>^H<char>` redraw no longer leaves a duplicated character
  (this is also what zsh's ZLE does — it was visible as `uuname -a` before the
  fix).
* **Prompt**: the script waits on `[>#$]`, so the CLI may not emit a normal shell
  prompt. Pass a custom regex via the `prompt` argument to `connect`, or set
  `prompt = "..."` in `hosts.toml`.
  *Without a matching prompt regex every command burns the full timeout (30 s)
  and returns `timed_out: true`.*
  **The regex must be end-anchored** (`\s*$`). A device that redraws its prompt
  while accepting the previous line (zsh does, and so do several switch CLIs)
  puts the prompt text and the echoed command on the *same line* — a regex that
  merely *contains* the prompt matches that line too, the read ends before the
  command's output arrives, and the leftover is then reported as the **next**
  command's output.
  * zsh (user@host path %): `[\w.-]+@[\w.-]+ [^\r\n]*%\s*$`
  * iSAM / typical CLI: `[#>$]\s*$`
  * default (most switches): `[\w.@:~()/\\-]+[#>$]\s*$`
* **Pagination**: `--More--` pages are auto-advanced by the tool; `terminal
  length 0` avoids it entirely.
* **Timeouts**: `run_command` waits for the prompt; if the prompt never matches
  the call times out and returns the output collected so far.

## 11. Authentication ladder (why a password may not be needed)

`connect` / `ssh_connect` try, in order: **password → private key files →
ssh-agent**. The key files come from `ssh -G <host>`, which always lists the
OpenSSH **default** identities (`~/.ssh/id_rsa`, `~/.ssh/id_ed25519`, …) even
for a bare IP address. So on a machine where your key is already authorised
for that account, key auth succeeds and the password is never used — a wrong
password can still “succeed”. Use `use_agent: false` to stop the agent, and
remember that the default identity files are still offered.
