# SSH-Connect

A Rust [Model Context Protocol](https://modelcontextprotocol.io) (MCP) server for
remote server administration and interactive network-device / console management over
**SSH, Telnet, and Serial**, with a set of bundled skills and agents.

SSH-Connect is the Rust successor to the TypeScript `SSH-MCP` tool. It combines two
earlier Rust projects into one binary:

- **Server operations** — Linux/Ubuntu administration (nginx, SSL/certbot, UFW,
  packages, deployment, systemd, MySQL/MariaDB, Redis, Docker, logs, processes, files,
  cron). Each command runs on its own SSH channel and reports an exit code; file
  transfer uses SFTP; optional trust-on-first-use host-key checking is available.
- **Interactive console** — prompt-aware management of switches, routers, and consoles
  over SSH (PTY), Telnet, and Serial, with pagination handling, enable/login flows, and
  fleet commands. A broker (Windows named pipe / Unix domain socket) lets multiple MCP
  clients share one set of live sessions and serial ports.

Built on the `rmcp` MCP SDK. SSH is pure Rust via `russh` (no OpenSSL/C dependency).

**Download:** prebuilt binaries for Linux, macOS, and Windows are on the
[Releases page](https://github.com/mixelpixx/SSH-Connect/releases/latest) — no Rust
toolchain required to run them.

## Two interaction models, one server

| Goal | Tool family | Connect with | Run with |
|------|-------------|--------------|----------|
| Linux/VPS administration with exit codes | server-ops | `ssh_connect` | `ssh_exec`, `vps_*`, `ubuntu_*` |
| Switches, routers, consoles, serial | interactive console | `connect` | `run_command`, `enable`, `login`, etc. |

Both families run in one process and share the broker, so a session opened by one MCP
client is usable from another.

## Tools

- **Session / discovery (interactive):** `connect`, `disconnect`, `list_sessions`,
  `list_com_ports`, `list_hosts`
- **Interactive command:** `run_command`, `run_commands`, `enable`, `login`,
  `expect_send`, `run_on_fleet`, `upload_config`, `download_config`
- **Switch convenience:** `switch_backup_config`, `switch_network_diagnostics`
- **SSH server-ops:** `ssh_connect`, `ssh_exec`, `ssh_upload_file`, `ssh_download_file`,
  `ssh_list_files`, `ssh_disconnect`
- **VPS / Ubuntu:** `vps_system_stats`, `vps_logs`, `vps_process`, `vps_file_read`,
  `vps_file_ops`, `vps_cron`, `ubuntu_service_control`, `vps_nginx_config`,
  `ubuntu_nginx_control`, `ubuntu_update_packages`, `ubuntu_ssl_certificate`,
  `ubuntu_website_deployment`, `ubuntu_ufw_firewall`, `ubuntu_mysql`, `vps_redis`,
  `vps_docker`, `vps_health_check`

On timeout, `ssh_exec` (and the `vps_*`/`ubuntu_*` tools) return an error and make a
best-effort attempt to terminate the remote command. The attempt is not guaranteed —
OpenSSH may ignore a signal on a non-PTY exec channel. `ubuntu_update_packages`,
`ubuntu_ssl_certificate`, and `vps_docker` accept an optional `timeoutMs`. For
operations that can run for minutes, run them detached and poll a log file rather than
relying on a long timeout (see the `vps-management` skill).

## Skills (`skills/`)

- **switch-management** — operational guidance for switches/routers: vendor idioms,
  pagination, enable/config, saving, fleet operations, firmware upgrades, console-to-SSH
  setup.
- **vps-management** — safe-change discipline and standard workflows (patch + reboot,
  SSL/certbot troubleshooting, service recovery, long-running-operation polling).
- **vps-health-report** — a standardized HTML server health report (plus JSON sidecar
  and delta), driven by `vps_health_check`.
- **health-report** — a standardized HTML network-device health report (plus JSON
  sidecar).
- **ssh-connect** — how to drive this MCP server itself: console vs server tool
  families, credential precedence, prompt regexes, transcript log, auth ladder.
- **ssh-connect-legacy-devices** — playbook for old gear (Alcatel iSAM OLTs, 2015-era
  IOS-XE): legacy algorithm negotiation, exec-vs-shell channels, the `^H` erase
  character, pagers, and a failure → cause → fix table.

### Client-side skills

Skills that guide the *agent* using this server live in the MCP client's own skills
directory — `~/.config/opencode/skills/<name>/SKILL.md` for opencode,
`~/.claude/skills/<name>/SKILL.md` for Claude Code. A `SKILL.md` needs YAML front
matter with `name` and `description`:

```markdown
---
name: ssh-connect
description: Manage remote servers, network devices and consoles over SSH/Telnet/Serial via the ssh-connect MCP server. Use when asked to connect to a host, run commands on a switch/router/OLT, or reach a legacy device that only supports old SSH algorithms.
---

# ssh-connect
…
```

Two ready-to-use skills for the legacy-device workflow (`ssh-connect` and
`ssh-connect-legacy-devices`) ship in this repository under `skills/`; copy the folder
into the client's skills directory and adjust the `description` to match your own
tools.

## Agents (`.claude/agents/`)

`vps-troubleshooter`, `switch-config-auditor`, `deployment-runner`,
`fleet-health-reporter`, `vps-health-reporter`.

## Build

Requires a recent stable Rust toolchain. Rust dependencies (including `rmcp = "1.8"`
from crates.io) are fetched by Cargo.

```sh
cargo build --release
# Binary: target/release/ssh-connect  (ssh-connect.exe on Windows)
```

Windows and macOS need no system packages. On **Linux**, the `serialport` crate
(used for `list_com_ports` and serial console access) needs `libudev` at build time:

```sh
sudo apt-get install -y libudev-dev pkg-config   # Debian/Ubuntu
```

Prefer not to build from source? Prebuilt binaries for Linux, macOS, and Windows are
attached to every tagged release on the
[Releases page](https://github.com/mixelpixx/SSH-Connect/releases/latest) (produced by
the `release` GitHub Actions workflow).

## Register with an MCP client

Claude Code:

```sh
claude mcp add ssh-connect --scope user -- /path/to/ssh-connect/target/release/ssh-connect
```

Or in an MCP settings file:

```json
{
  "mcpServers": {
    "ssh-connect": {
      "command": "/path/to/ssh-connect/target/release/ssh-connect"
    }
  }
}
```

The server speaks MCP over stdio; all logging goes to stderr (set `RUST_LOG=info` to see
broker/role messages).

### opencode

opencode keeps MCP servers under `mcp.servers` in `~/.config/opencode/opencode.json`.
The `environment` block is where the server's environment variables go — they are
applied to the server process, exactly like `env:` in a Claude Code config:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "servers": {
      "ssh-connect": {
        "type": "local",
        "command": [
          "/path/to/ssh-connect/target/release/ssh-connect"
        ],
        "environment": {
          "SSHCONNECT_HOSTS": "/path/to/ssh-connect/hosts.toml",
          "SSHCONNECT_SESSION_LOG_DIR": "/Users/you/.ssh-connect/logs"
        }
      }
    }
  }
}
```

Or from the shell (writes the same entry):

```sh
opencode mcp add ssh-connect --global \
  --env SSHCONNECT_HOSTS=/path/to/hosts.toml \
  --env SSHCONNECT_SESSION_LOG_DIR="$HOME/.ssh-connect/logs" \
  -- /path/to/ssh-connect/target/release/ssh-connect
```

### Claude Desktop / Claude Code (`mcpServers`)

The `env` key is the equivalent of opencode's `environment`:

```json
{
  "mcpServers": {
    "ssh-connect": {
      "command": "/path/to/ssh-connect/target/release/ssh-connect",
      "env": {
        "SSHCONNECT_HOSTS": "/path/to/hosts-connect/hosts.toml",
        "SSHCONNECT_SESSION_LOG_DIR": "/Users/you/.ssh-connect/logs",
        "SSHCONNECT_HOST_KEY_CHECK": "tofu"
      }
    }
  }
}
```

## Environment variables

All configuration is environment variables on the server process — there is no config
file to parse besides the host inventory.

| Variable | Default | Effect |
|---|---|---|
| `SSHCONNECT_HOSTS` | `hosts.toml` in the current working directory | Path to the host inventory (see `hosts.example.toml`). A missing file yields an empty inventory, not an error. |
| `SSHCONNECT_<STEM>_<FIELD>` | — | Per-host override from the environment. `<STEM>` is the host name upper-cased with non-alphanumerics collapsed to `_` (`core-sw-01` → `CORE_SW_01`). Fields: `PASSWORD`, `USERNAME`, `HOST`, `PORT`, `KEY_PATH`. |
| `SSHCONNECT_SESSION_LOG` | on | Set to `off` (or `0`/`false`/`no`/`disabled`) to disable the session transcript log entirely. |
| `SSHCONNECT_SESSION_LOG_DIR` | `~/.ssh-connect/logs` | Directory for session transcripts (`<session>.log`). |
| `SSHCONNECT_HOST_KEY_CHECK` | off (accept any key) | Set to `tofu` for trust-on-first-use against `~/.ssh-connect/known_hosts`; a later key mismatch is refused. The legacy name `VPS_HOST_KEY_CHECK` is also accepted. |
| `SSH_AUTH_SOCK` | — | Unix ssh-agent socket used for agent authentication (on Windows the OpenSSH named pipe `\\.\pipe\openssh-ssh-agent` is used when unset). |
| `XDG_RUNTIME_DIR` | temp dir | Where the broker's Unix domain socket is created. |
| `RUST_LOG` | — | Log level (`info`, `debug`, …). Logs go to stderr, never to stdout (stdout is the MCP channel). |

Secrets are best passed through the environment (`SSHCONNECT_<STEM>_PASSWORD`) or an
ssh-agent identity rather than written into `hosts.toml`.

## Configuration

Everything is configured through environment variables — see the
[Environment variables](#environment-variables) table above for the full list.

- **Inventory:** copy `hosts.example.toml` to `hosts.toml` and edit. The `connect` tool
  can then reach a device by `name` alone. Override the path with `SSHCONNECT_HOSTS`.
- **Secrets:** prefer environment variables over committing passwords. For host
  `core-sw-01`, set `SSHCONNECT_CORE_SW_01_PASSWORD` (name upper-cased, non-alphanumerics
  collapsed to `_`); the same pattern applies to `USERNAME`, `HOST`, `PORT`, `KEY_PATH`.
- **Host-key checking (server-ops SSH):** accepts any key by default. Set
  `SSHCONNECT_HOST_KEY_CHECK=tofu` (the legacy `VPS_HOST_KEY_CHECK` is also accepted) to
  enable trust-on-first-use against `~/.ssh-connect/known_hosts`, which refuses a later
  key mismatch. The interactive-console path (`connect`) does not pin host keys.
- **Session transcript log:** every interactive session records all wire traffic in both
  directions to `~/.ssh-connect/logs/<session>.log` (control characters escaped, so the
  file stays readable). The path is returned by `connect` and `list_sessions`. Set
  `SSHCONNECT_SESSION_LOG=off` to disable it or `SSHCONNECT_SESSION_LOG_DIR` to relocate
  it. Secrets are **not** masked — treat these files as sensitive.
- **Legacy SSH devices:** old network gear (Alcatel-Lucent iSAM OLTs, 2015-era IOS-XE,
  old appliances) that only offer SHA-1 key exchange, `ssh-rsa`/`ssh-dss` host keys, CBC
  ciphers and `hmac-sha1` are negotiated automatically — modern algorithms stay first in
  every list, legacy ones are used only when the device offers nothing else. The
  `dsa` feature of `russh` must stay enabled, otherwise `ssh-dss` is advertised but
  cannot be verified and the handshake fails with "unsupported algorithm: ssh-dss".
- **SSH client config:** resolution is delegated to the local OpenSSH client — both
  connect tools run `ssh -G <host>` and use the HostName, User, Port and IdentityFile it
  reports, so `Host`/`Match` blocks, `Include`s, wildcards, percent tokens,
  canonicalization and OpenSSH's own defaults (User = the local account, Port 22) all
  behave exactly as they do for `ssh` itself. Explicit arguments and `hosts.toml` entries
  still win. With no `ssh` on PATH, only the local account name is inferred.
- **ssh-agent:** identities held by the agent are offered after any password and key file.
  On Unix the socket comes from `SSH_AUTH_SOCK`; on Windows the OpenSSH named pipe
  (`\\.\pipe\openssh-ssh-agent`) is used when that variable is unset. Pass
  `useAgent: false` (`use_agent` on `connect`) to force a specific credential instead.
  RSA identities are signed with the strongest hash the server advertises, so agent auth
  also works against OpenSSH 8.8+, which rejects SHA-1 `ssh-rsa`.
- **Prompt regex:** `run_command` ends when the device's prompt matches the pattern, so
  the pattern must be **end-anchored** (`\s*$`). A pattern that merely *contains* the
  prompt also matches the line where a device redraws its prompt and echoes the command
  at the same time — the read then ends before the output arrives and the leftover is
  reported as the *next* command's output. A pattern that does not match at all burns
  the full timeout and returns `timed_out: true`. Set it per host with `prompt = "..."`
  in `hosts.toml` or per call with `connect {prompt: "..."}`.

  | Device | Pattern |
  |---|---|
  | default (most switches) | `[\w.@:~()/\\-]+[#>$]\s*$` |
  | iSAM / typical CLI | `[#>$]\s*$` |
  | zsh (user@host path %) | `[\w.-]+@[\w.-]+ [^\r\n]*%\s*$` |

  The transcript log shows the exact prompt bytes — match against that, never guess.
- **Prompt/erase handling:** backspaces are applied with their real erase semantics, so
  a device that uses `^H` as its erase key (the Alcatel iSAM does) no longer duplicates
  the first character of a redrawn line.

## Broker (multi-instance session sharing)

The first instance becomes the owner of a per-machine rendezvous endpoint — a named
pipe (`\\.\pipe\ssh-connect-broker-v1`) on Windows, or a Unix domain socket under
`$XDG_RUNTIME_DIR` (falling back to the temp dir) on Linux/macOS — and holds the live
connections and COM ports. Later instances act as proxies that forward `tools/call` to
the owner, so a session opened in one client is usable from another and two clients
cannot collide on the same serial port. Tool schemas are served locally by every
instance. On a platform with neither transport, each instance is a standalone owner.

Limitation: if the owner process exits, existing proxies do not currently re-elect a new
owner mid-session — restart the affected client. A newly started instance always elects
correctly (and clears a stale socket).

**Restart the server after a rebuild.** The owner process is long-lived and keeps serving
the binary it was started with, so a rebuilt binary has no effect until the server
restarts (in opencode: `/reload`). A second instance started while an owner is running
becomes a proxy and forwards to the *old* owner — which is why a fix can appear to have
had no effect.

## Tests

```sh
cargo test --bin ssh-connect
```

Unit tests cover the telnet IAC parser, prompt/ANSI output handling, the host inventory,
`ssh -G` alias resolution, serial-port probing, and the legacy SSH algorithm set.
Protocol and broker behavior are checked by sending an `initialize` + `tools/list`
sequence to the binary and by running two instances to confirm owner/proxy election.

## Documentation

- `MCP_EXPLAINED.md` — how the server works: architecture, the two tool families,
  credential precedence, host-key policy, legacy algorithms, the transcript log, and
  device-specific notes.
- `LEGACY_SSH_TESTING.md` — how to test legacy-SSH support (fake legacy SSH server,
  per-algorithm isolation matrix, negative control).
- `LEGACY_SSH_HANDOFF.md` — historical handoff notes from the legacy-SSH work.

## License

MIT — see [LICENSE](LICENSE).
