//! Per-session transcript log — every printable byte sent to and received
//! from the device, in order, with direction markers and timestamps.
//!
//! This is the debugging counterpart to the cleaned-up `run_command` output:
//! the tool strips ANSI, the echoed command and the trailing prompt, so the
//! transcript is the only place where the *exact* bytes on the wire are kept.
//! It is what you want when a device echoes something unexpected, splits a
//! command across chunks, or never emits the prompt you expected.
//!
//! Enable/disable and location are controlled by the environment:
//!
//! * `SSHCONNECT_SESSION_LOG` — `off` disables it (default: on).
//! * `SSHCONNECT_SESSION_LOG_DIR` — directory for the transcripts
//!   (default: `~/.ssh-connect/logs`).
//!
//! Secrets are *not* masked: the transcript records exactly what crossed the
//! wire, which is the point — but treat the files as sensitive.

use crate::error::ToolResult;
use async_trait::async_trait;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{Protocol, Transport};

/// A session's transcript file, shared by the transport wrapper.
#[derive(Clone)]
pub struct SessionLog {
    path: PathBuf,
    file: Arc<Mutex<File>>,
}

impl SessionLog {
    /// Open (create) the transcript for `name`, or `None` when logging is off
    /// or the file cannot be created (logging must never break a connection).
    pub fn open(name: &str) -> Option<Self> {
        if logging_disabled() {
            return None;
        }
        let dir = log_dir();
        if std::fs::create_dir_all(&dir).is_err() {
            return None;
        }
        // Keep the name filesystem-safe but recognisable.
        let safe: String = name
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        let path = dir.join(format!("{}.log", safe));
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()?;
        let log = Self { path, file: Arc::new(Mutex::new(file)) };
        log.note("--- session transcript opened ---");
        Some(log)
    }

    /// Absolute path of the transcript file.
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Record a direction-tagged chunk of raw bytes.
    pub fn record(&self, dir: Direction, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let text = render(data);
        let stamp = timestamp();
        let mut buf = String::with_capacity(text.len() + 32);
        buf.push_str(&stamp);
        buf.push(' ');
        buf.push_str(dir.label());
        buf.push(' ');
        buf.push_str(&text);
        buf.push('\n');
        if let Ok(mut f) = self.file.lock() {
            let _ = f.write_all(buf.as_bytes());
            let _ = f.flush();
        }
    }

    /// Record a free-text note (session events, not wire bytes).
    pub fn note(&self, msg: &str) {
        let stamp = timestamp();
        let mut buf = String::with_capacity(msg.len() + 32);
        buf.push_str(&stamp);
        buf.push_str(" -- ");
        buf.push_str(msg);
        buf.push('\n');
        if let Ok(mut f) = self.file.lock() {
            let _ = f.write_all(buf.as_bytes());
            let _ = f.flush();
        }
    }
}

/// Which way the bytes travelled.
#[derive(Clone, Copy)]
pub enum Direction {
    /// Host → device (commands and keystrokes we send).
    Tx,
    /// Device → host (everything the device sends back).
    Rx,
}

impl Direction {
    fn label(&self) -> &'static str {
        match self {
            Direction::Tx => "TX",
            Direction::Rx => "RX",
        }
    }
}

/// Render raw bytes as a printable, single-line-ish transcript entry.
///
/// Printable ASCII and UTF-8 pass through unchanged so the log reads like the
/// device output. Control characters become escapes (`\r`, `\n`, `\t`, `\e`,
/// `\xNN`) so line structure is preserved without corrupting the file.
fn render(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len() + 8);
    for &b in data {
        match b {
            b'\r' => out.push_str("\\r"),
            b'\n' => out.push_str("\\n\n"),
            b'\t' => out.push_str("\\t"),
            0x1b => out.push_str("\\e"),
            0x20..=0x7e => out.push(b as char),
            b if b < 0x20 => out.push_str(&format!("\\x{:02x}", b)),
            b => out.push(b as char),
        }
    }
    out
}

/// Wall-clock timestamp with millisecond resolution.
fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let millis = now.subsec_millis();
    let days = secs / 86_400;
    let tod = secs % 86_400;
    // Minimal civil-date conversion (no chrono dependency needed).
    let (y, m, d) = civil_from_days(days as i64);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        y,
        m,
        d,
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60,
        millis
    )
}

/// Howard Hinnant's days-from-civil inverse — days since epoch → (y, m, d).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Is the transcript log switched off?
fn logging_disabled() -> bool {
    match std::env::var("SSHCONNECT_SESSION_LOG") {
        Ok(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "off" | "0" | "false" | "no" | "disabled"
        ),
        Err(_) => false,
    }
}

/// Where transcripts are written.
fn log_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("SSHCONNECT_SESSION_LOG_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".ssh-connect")
        .join("logs")
}

/// Wraps a transport so every byte crossing it is recorded.
pub struct LoggedTransport {
    inner: Box<dyn Transport>,
    log: SessionLog,
}

impl LoggedTransport {
    pub fn new(inner: Box<dyn Transport>, log: SessionLog) -> Self {
        Self { inner, log }
    }
}

#[async_trait]
impl Transport for LoggedTransport {
    async fn write_all(&mut self, data: &[u8]) -> ToolResult<()> {
        self.log.record(Direction::Tx, data);
        self.inner.write_all(data).await
    }

    async fn read_chunk(&mut self, timeout: std::time::Duration) -> ToolResult<Vec<u8>> {
        let chunk = self.inner.read_chunk(timeout).await?;
        self.log.record(Direction::Rx, &chunk);
        Ok(chunk)
    }

    async fn close(&mut self) -> ToolResult<()> {
        self.log.note("--- session transcript closed ---");
        self.inner.close().await
    }

    fn protocol(&self) -> Protocol {
        self.inner.protocol()
    }

    async fn upload_file(&mut self, local: &std::path::Path, remote: &str) -> ToolResult<u64> {
        self.inner.upload_file(local, remote).await
    }

    async fn download_file(&mut self, remote: &str, local: &std::path::Path) -> ToolResult<u64> {
        self.inner.download_file(remote, local).await
    }
}

/// Open a transcript for `name`, if logging is enabled.
pub fn open_session_log(name: &str) -> Option<SessionLog> {
    SessionLog::open(name)
}
