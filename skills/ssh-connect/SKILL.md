---
name: ssh-connect
description: Manage remote servers, network devices, OLTs and consoles over SSH/Telnet/Serial via the ssh-connect MCP server. Use when asked to connect to a host, run commands on a switch/router/OLT/VPS, back up a device config, check device or server health, transfer files over SFTP, or reach a legacy device that only supports old SSH algorithms (SHA-1 kex, ssh-rsa/ssh-dss host keys, CBC ciphers, hmac-sha1).
---

# ssh-connect

MCP server (`ssh-connect`) for remote administration over **SSH, Telnet and
Serial**. Rust, pure `russh` — no OpenSSH needed on the target.

## The one rule: pick the right tool family

| Goal | Connect | Run |
|---|---|---|
| Switch / router / OLT / console (stateful CLI, prompt-driven) | `connect` | `run_command`, `enable`, `login` |
| Linux/VPS admin, needs exit codes | `ssh_connect` | `ssh_exec`, `vps_*`, `ubuntu_*` |

**Device CLIs (switches, OLTs, firewalls) → use the console path.** They are
stateful (`configure …` → `exit all`), they paginate, they prompt, and many
reject exec channels outright. Only use `ssh_exec` for a real shell on a server.

## Console path (devices)

```sh
connect      {name|host, protocol, port, username, password|key_path, prompt, wake}
run_command  {session, command, timeout_secs}
run_commands {session, commands[]}
enable       {session, password}          # privileged mode
login        {session, username, password} # device-level user/pass prompt
expect_send  {session, expect, send}      # answer a [confirm]/Password: prompt
run_on_fleet {sessions[], command}        # same command across many devices
disconnect   {session}
list_sessions / list_hosts / list_com_ports
```

Workflow:

1. `connect` with `name` (from `hosts.toml`) or explicit host/port/user.
   Add `prompt` when the device's prompt is unusual — **without a matching
   prompt regex every command times out**, and a regex that is not
   **end-anchored** (`\s*$`) ends the read too early, so the command's output
   shows up under the *next* command.
2. `run_command` → returns `output`, `matched_prompt`, `timed_out`.
   Check `timed_out`: `true` means the prompt never matched, so the output may
   be truncated — fix the prompt regex before trusting the result.
3. `disconnect` when done.

Prompt regex examples (all end-anchored):

| Device | Regex |
|---|---|
| default (most switches) | `[\w.@:~()/\\-]+[#>$]\s*$` |
| iSAM / typical CLI | `[#>$]\s*$` |
| zsh (user@host path %) | `[\w.-]+@[\w.-]+ [^\r\n]*%\s*$` |

## Server path (Linux/VPS)

```sh
ssh_connect {host, port, username, password|private_key_path, passphrase}
ssh_exec    {connectionId, command, cwd, timeout}
ssh_upload_file / ssh_download_file / ssh_list_files
ssh_disconnect
```

`vps_*` / `ubuntu_*` tools operate on a `connectionId` from `ssh_connect`:
stats, logs, processes, files, cron, nginx, certbot, UFW, packages, MySQL,
Redis, Docker, and a `vps_health_check` battery.

## Credentials

Precedence: **explicit argument → `hosts.toml` → `~/.ssh/config`.**

* `hosts.toml` — per-host defaults so `connect {name: "olt-01"}` just works.
  Path override: `SSHCONNECT_HOSTS`.
* `~/.ssh/config` — resolved by asking the local `ssh` (`ssh -G <host>`), so
  `Host`/`Match`/`Include` blocks all behave exactly as for `ssh` itself.
* **Environment variables** for secrets: `SSHCONNECT_<NAME>_PASSWORD`
  (name upper-cased, non-alphanumerics → `_`). Also `_USERNAME`, `_HOST`,
  `_PORT`, `_KEY_PATH`.
* **ssh-agent** identities are offered last.

Auth ladder: password → key files → agent. `ssh -G` always lists the OpenSSH
*default* identity files, so on a host where your key is already authorised the
password is never used — a wrong password can still "succeed".

## Session transcript log (see the raw bytes)

Every session records all wire traffic, both directions:

```
~/.ssh-connect/logs/<session>.log
```

The path is returned by `connect` (`transcript`) and `list_sessions`. Control
characters are escaped (`\r`, `\n`, `\t`, `\e`, `\xNN`).

```
2026-11-12T11:09:32.240Z TX uname -a\n
2026-11-12T11:09:32.242Z RX u\x08unam
2026-11-12T11:09:32.243Z RX Darwin MacMini2018.local 24.6.0 …
```

Use it when output looks wrong — it is the only place the exact bytes are kept
(`run_command` strips ANSI, the echoed command and the trailing prompt).
**Secrets are not masked — treat these files as sensitive.**

Env: `SSHCONNECT_SESSION_LOG=off` to disable, `SSHCONNECT_SESSION_LOG_DIR` to
relocate.

Both are environment variables on the MCP server process, so in opencode they
live in `~/.config/opencode/opencode.json` under
`mcp.servers.ssh-connect.environment`:

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

Add `"SSHCONNECT_SESSION_LOG": "off"` to that block to disable transcripts
entirely (useful when you do not want secrets written to disk).

## Legacy / old devices

Old network gear (Alcatel-Lucent iSAM OLTs, 2015-era IOS-XE, old appliances)
only offer SHA-1 key exchange, `ssh-rsa`/`ssh-dss` host keys, CBC ciphers and
`hmac-sha1`. Those are negotiated automatically — modern algorithms stay first,
legacy ones are used only when the device offers nothing else:

| Category | Legacy fallbacks enabled |
|---|---|
| kex | group-exchange-sha1, group14-sha1, group1-sha1 |
| host key | ssh-rsa (SHA-1), ssh-dss (DSA) |
| cipher | aes-cbc, 3des-cbc |
| MAC | hmac-sha1, hmac-sha1-etm |

`arcfour`/RC4 is not implemented (out of scope). See the
`ssh-connect-legacy-devices` skill for the full device playbook.

## Safety

* Prefer read-only commands first (`show …`, `display …`); a `configure` block on
  a live device can drop it.
* `run_command` on a device sends the command verbatim — a `commit`-style CLI
  may apply changes immediately.
* Clean up: `disconnect` interactive sessions and `ssh_disconnect` server
  connections when finished.
* Long operations: prefer `run_on_fleet` (per-device isolation) over a shell
  loop; one failing device does not abort the rest.
