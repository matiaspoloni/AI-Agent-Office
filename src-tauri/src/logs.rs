//! Reading Agent Office's own log files for Diagnostics: the newest daily
//! file of the application log or of the provider log, its last lines, with
//! a minimum level. Only files in the log folder are read.

use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use ts_rs::TS;

/// Bytes read from the end of a log file.
const TAIL_BYTES: u64 = 512 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum LogKind {
    /// `app.log.*`: the application.
    App,
    /// `providers.log.*`: adapters and protocol traffic.
    Providers,
}

impl LogKind {
    fn prefix(self) -> &'static str {
        match self {
            LogKind::App => "app.log",
            LogKind::Providers => "providers.log",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "UPPERCASE")]
#[ts(export)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    fn parse(token: &str) -> Option<Self> {
        match token {
            "TRACE" => Some(Self::Trace),
            "DEBUG" => Some(Self::Debug),
            "INFO" => Some(Self::Info),
            "WARN" => Some(Self::Warn),
            "ERROR" => Some(Self::Error),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LogLine {
    /// Timestamp as written (UTC, RFC 3339).
    pub at: String,
    pub level: LogLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub target: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LogTail {
    /// The file read (none yet: nothing was logged).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub file: Option<String>,
    /// Oldest first.
    pub lines: Vec<LogLine>,
    /// Older lines exist that are not shown.
    pub truncated: bool,
}

/// Parses one line written by the `tracing` formatter:
/// `2026-09-28T10:00:00.123456Z  INFO agent_office_lib::host: message k=v`.
/// Returns `None` for continuation lines (multi-line messages).
pub fn parse_line(line: &str) -> Option<LogLine> {
    let line = line.trim_end();
    let (at, rest) = line.split_once(char::is_whitespace)?;
    if !(at.len() >= 20 && at.as_bytes()[0].is_ascii_digit() && at.contains('T')) {
        return None;
    }
    let rest = rest.trim_start();
    let (level, rest) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let level = LogLevel::parse(level)?;
    let rest = rest.trim_start();
    // "target: message" when the part before ": " is one word (a module path,
    // possibly after span names).
    let (target, message) = match rest.split_once(": ") {
        Some((head, tail)) if !head.contains(' ') && !head.is_empty() => {
            (Some(head.to_owned()), tail.to_owned())
        }
        _ => (None, rest.to_owned()),
    };
    Some(LogLine {
        at: at.to_owned(),
        level,
        target,
        message,
    })
}

/// The newest `app.log.*` / `providers.log.*` file in `dir`.
pub fn newest_file(dir: &Path, kind: LogKind) -> Option<PathBuf> {
    let prefix = kind.prefix();
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name == prefix || name.starts_with(&format!("{prefix}."))
        })
        .filter_map(|e| {
            let modified = e.metadata().ok()?.modified().ok()?;
            Some((modified, e.file_name(), e.path()))
        })
        // Newest first; the date in the name breaks ties.
        .max_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)))
        .map(|(_, _, path)| path)
}

/// The last `max_lines` lines at `min_level` or above of the newest file.
pub fn read_tail(dir: &Path, kind: LogKind, max_lines: usize, min_level: LogLevel) -> LogTail {
    let Some(file) = newest_file(dir, kind) else {
        return LogTail::default();
    };
    let mut text = String::new();
    let mut cut_start = false;
    if let Ok(mut f) = std::fs::File::open(&file) {
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if len > TAIL_BYTES {
            let _ = f.seek(SeekFrom::Start(len - TAIL_BYTES));
            cut_start = true;
        }
        let mut bytes = Vec::new();
        let _ = f.read_to_end(&mut bytes);
        text = String::from_utf8_lossy(&bytes).into_owned();
    }
    let mut body = text.as_str();
    if cut_start {
        // Drop the partial first line.
        body = body.split_once('\n').map_or("", |(_, rest)| rest);
    }
    let mut lines: Vec<LogLine> = Vec::new();
    for raw in body.lines() {
        match parse_line(raw) {
            Some(line) => lines.push(line),
            None => {
                if let Some(last) = lines.last_mut() {
                    if !raw.trim().is_empty() {
                        last.message.push('\n');
                        last.message.push_str(raw.trim_end());
                    }
                }
            }
        }
    }
    lines.retain(|l| l.level >= min_level);
    let truncated = cut_start || lines.len() > max_lines;
    if lines.len() > max_lines {
        lines.drain(..lines.len() - max_lines);
    }
    LogTail {
        file: Some(file.display().to_string()),
        lines,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracing_lines_are_parsed() {
        let line = parse_line(
            "2026-09-28T10:00:00.123456Z  INFO agent_office_lib::host: host started data_dir=C:\\x providers=4",
        )
        .unwrap();
        assert_eq!(line.level, LogLevel::Info);
        assert_eq!(line.target.as_deref(), Some("agent_office_lib::host"));
        assert_eq!(line.message, "host started data_dir=C:\\x providers=4");
        let warn = parse_line(
            "2026-09-28T10:00:01.000000Z  WARN provider: provider error provider=\"codex\"",
        )
        .unwrap();
        assert_eq!(warn.level, LogLevel::Warn);
        assert_eq!(warn.target.as_deref(), Some("provider"));
        // No target (a message with spaces before the first ": ").
        let odd = parse_line("2026-09-28T10:00:02Z ERROR something went wrong: here").unwrap();
        assert_eq!(odd.target, None);
        assert_eq!(odd.message, "something went wrong: here");
        assert!(parse_line("   at src/main.rs:10").is_none());
        assert!(parse_line("not a log line").is_none());
    }

    #[test]
    fn newest_file_last_lines_and_levels() {
        let dir = std::env::temp_dir().join(format!("ao-logs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(
            read_tail(&dir, LogKind::App, 10, LogLevel::Trace),
            LogTail::default()
        );
        std::fs::write(
            dir.join("app.log.2026-09-27"),
            "2026-09-27T09:00:00Z  INFO a: old day\n",
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let mut today = String::new();
        for i in 0..30 {
            let level = if i % 10 == 0 { "WARN" } else { "INFO" };
            today.push_str(&format!("2026-09-28T10:00:{i:02}Z  {level} a: line {i}\n"));
        }
        today.push_str("2026-09-28T10:01:00Z ERROR a: failed\n  caused by: disk full\n");
        std::fs::write(dir.join("app.log.2026-09-28"), &today).unwrap();
        std::fs::write(
            dir.join("providers.log.2026-09-28"),
            "2026-09-28T10:00:00Z DEBUG provider::claude: stderr: hi\n",
        )
        .unwrap();

        let tail = read_tail(&dir, LogKind::App, 5, LogLevel::Trace);
        assert!(tail.file.unwrap().ends_with("app.log.2026-09-28"));
        assert_eq!(tail.lines.len(), 5);
        assert!(tail.truncated);
        let last = tail.lines.last().unwrap();
        assert_eq!(last.level, LogLevel::Error);
        assert_eq!(last.message, "failed\n  caused by: disk full");

        let warnings = read_tail(&dir, LogKind::App, 100, LogLevel::Warn);
        let messages: Vec<&str> = warnings.lines.iter().map(|l| l.message.as_str()).collect();
        assert_eq!(
            messages,
            [
                "line 0",
                "line 10",
                "line 20",
                "failed\n  caused by: disk full"
            ]
        );
        assert!(!warnings.truncated);

        let providers = read_tail(&dir, LogKind::Providers, 100, LogLevel::Trace);
        assert_eq!(
            providers.lines[0].target.as_deref(),
            Some("provider::claude")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
