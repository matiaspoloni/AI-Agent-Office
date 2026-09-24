//! Process manager for managed agent sessions.
//!
//! * Spawns without a console window, with piped stdin/stdout/stderr.
//! * Owns the **whole process tree** of each child: a Windows Job Object with
//!   `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, or a Unix process group. Killing the
//!   tree kills the agent and every shell it started — and nothing else.
//! * Reports output line by line and the exit status as [`ProcessEvent`]s.
//!
//! Agent Office never uses this to touch a process it did not start.

mod tree;

use ao_core::time::now_ms;
use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitInfo {
    pub code: Option<i32>,
    pub success: bool,
    /// True when Agent Office killed the process tree.
    pub killed: bool,
    pub at_ms: i64,
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
    stdin: Mutex<Option<ChildStdin>>,
    tree: tree::ProcessTree,
    exit: watch::Receiver<Option<ExitInfo>>,
    kill_tx: mpsc::UnboundedSender<()>,
    /// Set before the tree is signalled, so an exit caused by our kill is
    /// reported as killed even when it is noticed before the kill request.
    kill_requested: Arc<AtomicBool>,
}

async fn pump_lines<R>(reader: R, stream: Stream, tx: mpsc::Sender<ProcessEvent>)
where
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
        let tree = tree::ProcessTree::attach(&child, pid);

        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let (events_tx, events_rx) = mpsc::channel(4096);
        let (exit_tx, exit_rx) = watch::channel(None);
        let (kill_tx, mut kill_rx) = mpsc::unbounded_channel::<()>();
        let kill_requested = Arc::new(AtomicBool::new(false));
        let kill_flag = kill_requested.clone();

        let readers: Vec<_> = [
            stdout.map(|s| tokio::spawn(pump_lines(s, Stream::Stdout, events_tx.clone()))),
            stderr.map(|s| tokio::spawn(pump_lines(s, Stream::Stderr, events_tx.clone()))),
        ]
        .into_iter()
        .flatten()
        .collect();

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
            // Let the readers drain what is left; grandchildren may keep the
            // pipes open, so don't wait forever.
            for reader in readers {
                let _ = tokio::time::timeout(Duration::from_secs(2), reader).await;
            }
            let info = match status {
                Ok(status) => ExitInfo {
                    code: status.code(),
                    success: status.success(),
                    killed,
                    at_ms: now_ms(),
                },
                Err(err) => {
                    tracing::warn!(%err, pid, "failed to wait for child");
                    ExitInfo {
                        code: None,
                        success: false,
                        killed,
                        at_ms: now_ms(),
                    }
                }
            };
            let _ = exit_tx.send(Some(info.clone()));
            let _ = events_tx.send(ProcessEvent::Exited(info)).await;
        });

        Ok((
            Arc::new(ManagedProcess {
                pid,
                started_at_ms: now_ms(),
                stdin: Mutex::new(stdin),
                tree,
                exit: exit_rx,
                kill_tx,
                kill_requested,
            }),
            events_rx,
        ))
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
                        code: None,
                        success: false,
                        killed: false,
                        at_ms: now_ms(),
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
                code: None,
                success: false,
                killed: true,
                at_ms: now_ms(),
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
