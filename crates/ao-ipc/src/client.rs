//! Blocking client used by the hook relay. Deliberately small: it must start
//! and finish in milliseconds and never hang an agent CLI.

use crate::frame::{read_frame, write_frame};
use crate::{Endpoint, HookRequest, HookResponse};
use std::io;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub enum ClientError {
    /// No app is listening (normal when Agent Office is closed).
    NotRunning,
    Timeout,
    Io(io::Error),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::NotRunning => f.write_str("Agent Office is not running"),
            ClientError::Timeout => f.write_str("timed out waiting for Agent Office"),
            ClientError::Io(e) => write!(f, "i/o error: {e}"),
        }
    }
}

#[cfg(unix)]
type Conn = std::os::unix::net::UnixStream;
#[cfg(windows)]
type Conn = std::fs::File;

#[cfg(unix)]
fn connect(endpoint: &Endpoint, _deadline: Instant) -> Result<Conn, ClientError> {
    match std::os::unix::net::UnixStream::connect(&endpoint.socket_path) {
        Ok(stream) => Ok(stream),
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            Err(ClientError::NotRunning)
        }
        Err(e) => Err(ClientError::Io(e)),
    }
}

#[cfg(windows)]
fn connect(endpoint: &Endpoint, deadline: Instant) -> Result<Conn, ClientError> {
    const ERROR_FILE_NOT_FOUND: i32 = 2;
    const ERROR_PIPE_BUSY: i32 = 231;
    loop {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&endpoint.pipe_name)
        {
            Ok(file) => return Ok(file),
            Err(e) if e.raw_os_error() == Some(ERROR_FILE_NOT_FOUND) => {
                return Err(ClientError::NotRunning)
            }
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                if Instant::now() >= deadline {
                    return Err(ClientError::Timeout);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(ClientError::Io(e)),
        }
    }
}

/// Sends one request and waits for the response.
///
/// The read happens on a helper thread so the timeout works for every
/// transport (Windows pipe handles opened as files have no read timeout).
pub fn call(
    endpoint: &Endpoint,
    request: &HookRequest,
    connect_timeout: Duration,
    response_timeout: Duration,
) -> Result<HookResponse, ClientError> {
    let mut conn = connect(endpoint, Instant::now() + connect_timeout)?;
    write_frame(&mut conn, request).map_err(ClientError::Io)?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(read_frame::<HookResponse>(&mut conn));
    });
    match rx.recv_timeout(response_timeout) {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(e)) => Err(ClientError::Io(e)),
        Err(_) => Err(ClientError::Timeout),
    }
}
