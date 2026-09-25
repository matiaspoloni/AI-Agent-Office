//! Process manager for managed agent sessions.
//!
//! * Spawns without a console window, with piped stdin/stdout/stderr.
//! * Owns the **whole process tree** of each child: a Windows Job Object with
//!   `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, or a Unix process group. Killing the
//!   tree kills the agent and every shell it started — and nothing else.
//! * On Windows the child starts suspended and runs only once it is inside its
//!   job, so nothing it starts can escape the tree.
//! * Reports output line by line and the exit status as [`ProcessEvent`]s;
//!   processes a finished child leaves running in its tree are stopped and
//!   counted ([`ExitInfo::leftovers`]).
//! * Keeps a registry of the processes it started ([`managed_processes`]) for
//!   Diagnostics.
//!
//! Agent Office never uses this to touch a process it did not start.

mod tree;

use ao_core::time::now_ms;
use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{mpsc, watch, Mutex};

pub use tree::is_process_alive;

/// Lines longer than this are truncated (protects memory against runaway output).
pub const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Default)]
pub struct SpawnSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(OsString, OsString)>,
}

impl SpawnSpec {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            ..Default::default()
        }
    }

    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExitInfo {
    pub code: Option<i32>,
    /// Unix: the signal that ended the process.
    pub signal: Option<i32>,
    pub success: bool,
    /// True when Agent Office killed the process tree.
    pub killed: bool,
    /// Processes the child left running in its tree when it exited; Agent
    /// Office stopped them (they belong to the finished session).
    pub leftovers: u32,
    pub at_ms: i64,
}

/// Windows exit codes that are NTSTATUS values worth naming.
fn windows_status(code: u32) -> Option<&'static str> {
    Some(match code {
        0xC000_0005 => "crashed (access violation)",
        0xC000_00FD => "crashed (stack overflow)",
        0xC000_0409 => "crashed (security check failure)",
        0xC000_001D => "crashed (illegal instruction)",
        0xC000_0094 => "crashed (division by zero)",
        0xC000_0135 => "could not start (a required DLL is missing)",
        0xC000_0142 => "could not start (initialization failed)",
        0xC000_013A => "interrupted (Ctrl+C)",
        0xC000_0017 => "ran out of memory",
        _ => return None,
    })
}

fn unix_signal(signal: i32) -> &'static str {
    match signal {
        1 => "hung up (SIGHUP)",
        2 => "interrupted (SIGINT)",
        6 => "aborted (SIGABRT)",
        9 => "killed (SIGKILL)",
        11 => "crashed (segmentation fault)",
        13 => "broken pipe (SIGPIPE)",
        15 => "terminated (SIGTERM)",
        _ => "ended by a signal",
    }
}

impl ExitInfo {
    /// A short, human-readable account of how the process ended, e.g.
    /// "finished", "exited with code 2", "crashed (access violation)".
    pub fn describe(&self) -> String {
        let mut text = if self.killed {
            "stopped by Agent Office".to_owned()
        } else if self.success {
            "finished".to_owned()
        } else if let Some(signal) = self.signal {
            format!("{} (signal {signal})", unix_signal(signal))
        } else if let Some(code) = self.code {
            match windows_status(code as u32) {
                Some(name) => format!("{name}, code 0x{:08X}", code as u32),
                None => format!("exited with code {code}"),
            }
        } else {
            "exited with an error".to_owned()
        };
        if self.leftovers > 0 {
            let s = if self.leftovers == 1 { "" } else { "es" };
            text.push_str(&format!("; {} leftover process{s} stopped", self.leftovers));
        }
        text
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessEvent {
    Line { stream: Stream, line: String },
    Exited(ExitInfo),
}

/// A running (or finished) child process owned by Agent Office.
pub struct ManagedProcess {
    pid: u32,
    started_at_ms: i64,
    program: String,
    label: std::sync::Mutex<String>,
    stdin: Mutex<Option<ChildStdin>>,
    tree: Arc<tree::ProcessTree>,
    exit: watch::Receiver<Option<ExitInfo>>,
    kill_tx: mpsc::UnboundedSender<()>,
    /// Set before the tree is signalled, so an exit caused by our kill is
    /// reported as killed even when it is noticed before the kill request.
    kill_requested: Arc<AtomicBool>,
    /// When the child last wrote a line (ms since epoch; 0 = never).
    last_output_ms: Arc<AtomicI64>,
}

/// What Diagnostics shows about a process Agent Office started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSnapshot {
    pub pid: u32,
    pub label: String,
    pub program: String,
    pub started_at_ms: i64,
    pub exit: Option<ExitInfo>,
    pub last_output_ms: Option<i64>,
    /// Processes alive in its tree (the agent and what it started), when known.
    pub tree_processes: Option<u32>,
}

static REGISTRY: std::sync::Mutex<Vec<Weak<ManagedProcess>>> = std::sync::Mutex::new(Vec::new());

/// Every process started through this crate whose handle is still held.
pub fn managed_processes() -> Vec<ProcessSnapshot> {
    let mut registry = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    registry.retain(|w| w.strong_count() > 0);
    registry
        .iter()
        .filter_map(Weak::upgrade)
        .map(|p| p.snapshot())
        .collect()
}

#[cfg(unix)]
fn exit_signal(status: &std::process::ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    status.signal()
}

#[cfg(not(unix))]
fn exit_signal(_status: &std::process::ExitStatus) -> Option<i32> {
    None
}

async fn pump_lines<R>(
    reader: R,
    stream: Stream,
    tx: mpsc::Sender<ProcessEvent>,
    last_output: Arc<AtomicI64>,
) where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::with_capacity(8 * 1024);
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf).await {
            Ok(0) => break,
            Ok(_) => {
                while buf.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
                    buf.pop();
                }
                if buf.len() > MAX_LINE_BYTES {
                    buf.truncate(MAX_LINE_BYTES);
                }
                let line = String::from_utf8_lossy(&buf).into_owned();
                last_output.store(now_ms(), Ordering::Relaxed);
                if tx.send(ProcessEvent::Line { stream, line }).await.is_err() {
                    break;
                }
            }
            Err(err) => {
                tracing::debug!(%err, ?stream, "stopped reading child output");
                break;
            }
        }
    }
}

impl ManagedProcess {
    /// Spawns the process. Returns the handle and a receiver of its output
    /// lines followed by exactly one [`ProcessEvent::Exited`].
    pub fn spawn(
        spec: SpawnSpec,
    ) -> io::Result<(Arc<ManagedProcess>, mpsc::Receiver<ProcessEvent>)> {
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(false);
        if let Some(cwd) = &spec.cwd {
            cmd.current_dir(cwd);
        }
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        tree::prepare(&mut cmd);

        let mut child = cmd.spawn()?;
        let pid = child.id().unwrap_or_default();
        let tree = Arc::new(tree::ProcessTree::attach(&child, pid)?);
        let program = spec
            .program
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| spec.program.display().to_string());

        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let (events_tx, events_rx) = mpsc::channel(4096);
        let (exit_tx, exit_rx) = watch::channel(None);
        let (kill_tx, mut kill_rx) = mpsc::unbounded_channel::<()>();
        let kill_requested = Arc::new(AtomicBool::new(false));
        let kill_flag = kill_requested.clone();
        let last_output_ms = Arc::new(AtomicI64::new(0));

        let readers: Vec<_> = [
            stdout.map(|s| {
                tokio::spawn(pump_lines(
                    s,
                    Stream::Stdout,
                    events_tx.clone(),
                    last_output_ms.clone(),
                ))
            }),
            stderr.map(|s| {
                tokio::spawn(pump_lines(
                    s,
                    Stream::Stderr,
                    events_tx.clone(),
                    last_output_ms.clone(),
                ))
            }),
        ]
        .into_iter()
        .flatten()
        .collect();

        let exit_tree = tree.clone();
        tokio::spawn(async move {
            let mut killed = false;
            let status = loop {
                tokio::select! {
                    status = child.wait() => break status,
                    request = kill_rx.recv() => {
                        if request.is_some() {
                            killed = true;
                            let _ = child.start_kill();
                        }
                    }
                }
            };
            let killed = killed || kill_flag.load(Ordering::SeqCst);
            // Whatever the child left running in its tree belongs to it: stop
            // it (this also closes pipes a grandchild may still hold).
            let leftovers = if killed {
                0
            } else {
                exit_tree.stop_leftovers()
            };
            if leftovers > 0 {
                tracing::info!(
                    pid,
                    leftovers,
                    "stopped processes left running by a finished child"
                );
            }
            // Let the readers drain what is left; don't wait forever.
            for reader in readers {
                let _ = tokio::time::timeout(Duration::from_secs(2), reader).await;
            }
            let info = match status {
                Ok(status) => ExitInfo {
                    code: status.code(),
                    signal: exit_signal(&status),
                    success: status.success(),
                    killed,
                    leftovers,
                    at_ms: now_ms(),
                },
                Err(err) => {
                    tracing::warn!(%err, pid, "failed to wait for child");
                    ExitInfo {
                        killed,
                        leftovers,
                        at_ms: now_ms(),
                        ..Default::default()
                    }
                }
            };
            let _ = exit_tx.send(Some(info.clone()));
            let _ = events_tx.send(ProcessEvent::Exited(info)).await;
        });

        let process = Arc::new(ManagedProcess {
            pid,
            started_at_ms: now_ms(),
            label: std::sync::Mutex::new(program.clone()),
            program,
            stdin: Mutex::new(stdin),
            tree,
            exit: exit_rx,
            kill_tx,
            kill_requested,
            last_output_ms,
        });
        REGISTRY
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Arc::downgrade(&process));
        Ok((process, events_rx))
    }

    /// Names the process for Diagnostics (e.g. "Claude Code session 1a2b…").
    pub fn set_label(&self, label: impl Into<String>) {
        *self.label.lock().unwrap_or_else(|e| e.into_inner()) = label.into();
    }

    /// When the child last wrote a line, if ever.
    pub fn last_output_ms(&self) -> Option<i64> {
        Some(self.last_output_ms.load(Ordering::Relaxed)).filter(|t| *t > 0)
    }

    /// Processes alive in the child's tree, when the platform can tell.
    pub fn tree_process_count(&self) -> Option<u32> {
        self.tree.process_count()
    }

    pub fn snapshot(&self) -> ProcessSnapshot {
        let exit = self.exit_info();
        ProcessSnapshot {
            pid: self.pid,
            label: self.label.lock().unwrap_or_else(|e| e.into_inner()).clone(),
            program: self.program.clone(),
            started_at_ms: self.started_at_ms,
            last_output_ms: self.last_output_ms(),
            tree_processes: if exit.is_some() {
                Some(0)
            } else {
                self.tree_process_count()
            },
            exit,
        }
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn started_at_ms(&self) -> i64 {
        self.started_at_ms
    }

    pub fn exit_info(&self) -> Option<ExitInfo> {
        self.exit.borrow().clone()
    }

    pub fn is_running(&self) -> bool {
        self.exit_info().is_none()
    }

    /// Writes one line (a trailing newline is added) to the child's stdin.
    pub async fn write_line(&self, line: &str) -> io::Result<()> {
        let mut guard = self.stdin.lock().await;
        let stdin = guard
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "stdin is closed"))?;
        stdin.write_all(line.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await
    }

    /// Closes stdin (EOF). Many agent CLIs finish gracefully on end of input.
    pub async fn close_stdin(&self) {
        if let Some(mut stdin) = self.stdin.lock().await.take() {
            let _ = stdin.shutdown().await;
        }
    }

    /// Waits for the exit, up to `timeout`.
    pub async fn wait(&self, timeout: Duration) -> Option<ExitInfo> {
        let mut rx = self.exit.clone();
        let result = tokio::time::timeout(timeout, async {
            loop {
                if let Some(info) = rx.borrow().clone() {
                    return info;
                }
                if rx.changed().await.is_err() {
                    // Sender gone without a value: treat as exited.
                    return ExitInfo {
                        at_ms: now_ms(),
                        ..Default::default()
                    };
                }
            }
        })
        .await;
        result.ok()
    }

    /// Kills the process and every descendant in its tree. Safe to call twice.
    pub fn kill_tree(&self) {
        let running = self.is_running();
        if running {
            self.kill_requested.store(true, Ordering::SeqCst);
        }
        self.tree.terminate(running);
        let _ = self.kill_tx.send(());
    }

    /// Graceful stop: close stdin, wait `grace`, then kill the tree.
    pub async fn stop(&self, grace: Duration) -> ExitInfo {
        self.close_stdin().await;
        if let Some(info) = self.wait(grace).await {
            return info;
        }
        self.kill_tree();
        self.wait(Duration::from_secs(10))
            .await
            .unwrap_or(ExitInfo {
                killed: true,
                at_ms: now_ms(),
                ..Default::default()
            })
    }
}

impl Drop for ManagedProcess {
    fn drop(&mut self) {
        // Dropping the handle must not leave orphans behind.
        if self.is_running() {
            self.tree.terminate(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exit(code: Option<i32>) -> ExitInfo {
        ExitInfo {
            code,
            success: code == Some(0),
            ..Default::default()
        }
    }

    #[test]
    fn exits_are_described_in_plain_words() {
        assert_eq!(exit(Some(0)).describe(), "finished");
        assert_eq!(exit(Some(2)).describe(), "exited with code 2");
        assert_eq!(
            exit(Some(0xC000_0005_u32 as i32)).describe(),
            "crashed (access violation), code 0xC0000005"
        );
        assert_eq!(
            exit(Some(0xC000_013A_u32 as i32)).describe(),
            "interrupted (Ctrl+C), code 0xC000013A"
        );
        assert_eq!(
            ExitInfo {
                signal: Some(11),
                ..Default::default()
            }
            .describe(),
            "crashed (segmentation fault) (signal 11)"
        );
        assert_eq!(
            ExitInfo {
                killed: true,
                code: Some(1),
                ..Default::default()
            }
            .describe(),
            "stopped by Agent Office"
        );
        assert_eq!(
            ExitInfo {
                success: true,
                code: Some(0),
                leftovers: 2,
                ..Default::default()
            }
            .describe(),
            "finished; 2 leftover processes stopped"
        );
        assert_eq!(exit(None).describe(), "exited with an error");
    }
}
