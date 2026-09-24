//! Async server run by the app. One connection = one hook invocation.

use crate::frame::{read_frame_async, write_frame_async};
use crate::{tokens_match, Endpoint, HookRequest, HookResponse, PROTOCOL_VERSION};
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;

pub type Handler =
    Arc<dyn Fn(HookRequest) -> Pin<Box<dyn Future<Output = HookResponse> + Send>> + Send + Sync>;

/// Wraps an async closure into a [`Handler`].
pub fn handler<F, Fut>(f: F) -> Handler
where
    F: Fn(HookRequest) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = HookResponse> + Send + 'static,
{
    Arc::new(move |req| Box::pin(f(req)))
}

pub struct Server {
    task: JoinHandle<()>,
    endpoint: Endpoint,
}

impl Server {
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    pub fn is_running(&self) -> bool {
        !self.task.is_finished()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
        #[cfg(unix)]
        {
            let _ = std::fs::remove_file(&self.endpoint.socket_path);
        }
    }
}

async fn serve_connection<S>(mut stream: S, token: Arc<String>, handler: Handler)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let request = match tokio::time::timeout(
        Duration::from_secs(10),
        read_frame_async::<HookRequest>(&mut stream),
    )
    .await
    {
        Ok(Ok(request)) => request,
        Ok(Err(err)) => {
            tracing::debug!(%err, "dropping malformed hook request");
            return;
        }
        Err(_) => return,
    };
    if request.v != PROTOCOL_VERSION || !tokens_match(&request.token, &token) {
        tracing::warn!(provider = %request.provider, "rejected hook request with bad token or version");
        let _ = write_frame_async(&mut stream, &HookResponse::default()).await;
        return;
    }
    let response = handler(request).await;
    if let Err(err) = write_frame_async(&mut stream, &response).await {
        // The relay may have been killed (e.g. hook timeout); nothing to do.
        tracing::debug!(%err, "could not deliver hook response");
    }
}

#[cfg(unix)]
pub async fn start(endpoint: Endpoint, token: String, handler: Handler) -> io::Result<Server> {
    use std::os::unix::fs::PermissionsExt;
    use tokio::net::UnixListener;

    let path = endpoint.socket_path.clone();
    if path.exists() {
        // A live server answers; a stale socket file refuses connections.
        if std::os::unix::net::UnixStream::connect(&path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "another Agent Office instance is already listening",
            ));
        }
        std::fs::remove_file(&path)?;
    }
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    let token = Arc::new(token);
    let task = tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    tokio::spawn(serve_connection(stream, token.clone(), handler.clone()));
                }
                Err(err) => {
                    tracing::warn!(%err, "hook listener accept failed");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    });
    Ok(Server { task, endpoint })
}

#[cfg(windows)]
pub async fn start(endpoint: Endpoint, token: String, handler: Handler) -> io::Result<Server> {
    use tokio::net::windows::named_pipe::ServerOptions;

    let name = endpoint.pipe_name.clone();
    // `first_pipe_instance` fails if someone else already owns the name
    // (another Agent Office instance, or a process squatting on it).
    let mut server = ServerOptions::new()
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .create(&name)?;
    let token = Arc::new(token);
    let task = tokio::spawn(async move {
        loop {
            if let Err(err) = server.connect().await {
                tracing::warn!(%err, "hook pipe connect failed");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
            let connected = server;
            server = match ServerOptions::new()
                .reject_remote_clients(true)
                .create(&name)
            {
                Ok(next) => next,
                Err(err) => {
                    tracing::error!(%err, "could not create next hook pipe instance; stopping listener");
                    tokio::spawn(serve_connection(connected, token.clone(), handler.clone()));
                    return;
                }
            };
            tokio::spawn(serve_connection(connected, token.clone(), handler.clone()));
        }
    });
    Ok(Server { task, endpoint })
}
