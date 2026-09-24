//! Minimal JSON-RPC 2.0 client over the stdio of a managed process, one JSON
//! message per line (`codex app-server`, ACP agents such as `agent acp`).
//!
//! Tolerant by design: servers that omit the `"jsonrpc"` member (codex-cli
//! 0.156.1 does) or add extra members (`emittedAtMs`) are accepted.

use ao_process::ManagedProcess;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

#[derive(Debug, Clone, PartialEq)]
pub enum RpcError {
    /// The server answered with a JSON-RPC error.
    Server {
        code: i64,
        message: String,
    },
    Timeout(String),
    /// The process exited or stdin is closed.
    Closed,
    Io(String),
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RpcError::Server { code, message } => write!(f, "{message} (code {code})"),
            RpcError::Timeout(method) => write!(f, "no answer to `{method}` in time"),
            RpcError::Closed => write!(f, "the agent process is not running"),
            RpcError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// A message from the server that is not a response to one of our requests.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Notification {
        method: String,
        params: Value,
        emitted_at_ms: Option<i64>,
    },
    /// Server → client request (approvals and others); must be answered.
    Request {
        id: Value,
        method: String,
        params: Value,
    },
}

type Pending = Mutex<HashMap<String, oneshot::Sender<Result<Value, RpcError>>>>;

pub struct RpcClient {
    process: Arc<ManagedProcess>,
    next_id: AtomicI64,
    pending: Pending,
}

fn key(id: &Value) -> String {
    id.to_string()
}

impl RpcClient {
    pub fn new(process: Arc<ManagedProcess>) -> Arc<Self> {
        Arc::new(Self {
            process,
            next_id: AtomicI64::new(1),
            pending: Mutex::new(HashMap::new()),
        })
    }

    pub fn process(&self) -> &Arc<ManagedProcess> {
        &self.process
    }

    async fn send(&self, message: Value) -> Result<(), RpcError> {
        self.process
            .write_line(&message.to_string())
            .await
            .map_err(|e| {
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::NotConnected
                ) {
                    RpcError::Closed
                } else {
                    RpcError::Io(e.to_string())
                }
            })
    }

    pub async fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, RpcError> {
        self.request_until(method, params, Some(timeout)).await
    }

    /// Like [`request`](Self::request); `None` waits until the answer arrives
    /// or the process exits (long-running calls such as an ACP prompt turn).
    pub async fn request_until(
        &self,
        method: &str,
        params: Value,
        timeout: Option<Duration>,
    ) -> Result<Value, RpcError> {
        let id = json!(self.next_id.fetch_add(1, Ordering::Relaxed));
        let (tx, rx) = oneshot::channel();
        self.pending.lock().expect("rpc lock").insert(key(&id), tx);
        let sent = self
            .send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await;
        if let Err(e) = sent {
            self.pending.lock().expect("rpc lock").remove(&key(&id));
            return Err(e);
        }
        let Some(timeout) = timeout else {
            return rx.await.unwrap_or(Err(RpcError::Closed));
        };
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(RpcError::Closed),
            Err(_) => {
                self.pending.lock().expect("rpc lock").remove(&key(&id));
                Err(RpcError::Timeout(method.to_owned()))
            }
        }
    }

    pub async fn notify(&self, method: &str, params: Option<Value>) -> Result<(), RpcError> {
        let mut message = json!({ "jsonrpc": "2.0", "method": method });
        if let Some(params) = params {
            message["params"] = params;
        }
        self.send(message).await
    }

    pub async fn respond(&self, id: &Value, result: Value) -> Result<(), RpcError> {
        self.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
            .await
    }

    pub async fn respond_error(
        &self,
        id: &Value,
        code: i64,
        message: &str,
    ) -> Result<(), RpcError> {
        self.send(
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }),
        )
        .await
    }

    /// Parses one stdout line. Responses complete their request and return
    /// `None`; notifications and server requests are returned.
    pub fn handle_line(&self, line: &str) -> Option<Incoming> {
        let message: Value = serde_json::from_str(line.trim()).ok()?;
        let method = message.get("method").and_then(Value::as_str);
        let id = message.get("id").filter(|v| !v.is_null());
        match (method, id) {
            (Some(method), Some(id)) => Some(Incoming::Request {
                id: id.clone(),
                method: method.to_owned(),
                params: message.get("params").cloned().unwrap_or(Value::Null),
            }),
            (Some(method), None) => Some(Incoming::Notification {
                method: method.to_owned(),
                params: message.get("params").cloned().unwrap_or(Value::Null),
                emitted_at_ms: message.get("emittedAtMs").and_then(Value::as_i64),
            }),
            (None, Some(id)) => {
                let waiter = self.pending.lock().expect("rpc lock").remove(&key(id));
                if let Some(waiter) = waiter {
                    let result = match message.get("error") {
                        Some(error) => Err(RpcError::Server {
                            code: error.get("code").and_then(Value::as_i64).unwrap_or(0),
                            message: error
                                .get("message")
                                .and_then(Value::as_str)
                                .unwrap_or("unknown error")
                                .to_owned(),
                        }),
                        None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                    };
                    let _ = waiter.send(result);
                }
                None
            }
            (None, None) => None,
        }
    }

    /// Fails every request still waiting (the process exited).
    pub fn fail_all(&self) {
        let waiters: Vec<_> = self.pending.lock().expect("rpc lock").drain().collect();
        for (_, waiter) in waiters {
            let _ = waiter.send(Err(RpcError::Closed));
        }
    }
}
