#![cfg(feature = "server")]

use ao_ipc::client::{call, ClientError};
use ao_ipc::server::{handler, start};
use ao_ipc::{Endpoint, HookOrigin, HookRequest, HookResponse, PROTOCOL_VERSION};
use std::time::Duration;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "ao-ipc-{tag}-{}-{}",
        std::process::id(),
        ao_ipc::now_ms()
    ))
}

fn request(token: &str, event: &str) -> HookRequest {
    HookRequest {
        v: PROTOCOL_VERSION,
        token: token.into(),
        provider: "claude".into(),
        origin: HookOrigin::Global,
        received_at_ms: ao_ipc::now_ms(),
        relay_pid: std::process::id(),
        payload: serde_json::json!({ "hook_event_name": event }),
        payload_truncated: false,
    }
}

async fn blocking_call(
    endpoint: Endpoint,
    req: HookRequest,
    wait: Duration,
) -> Result<HookResponse, ClientError> {
    tokio::task::spawn_blocking(move || call(&endpoint, &req, Duration::from_millis(500), wait))
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn request_response_with_token_check() {
    let dir = temp_dir("rt");
    let token = ao_ipc::token::ensure_token(&dir).unwrap();
    let endpoint = Endpoint::for_data_dir(&dir);
    let server = start(
        endpoint.clone(),
        token.clone(),
        handler(|req: HookRequest| async move {
            HookResponse {
                stdout: Some(format!("seen {}", req.payload["hook_event_name"])),
                exit_code: 0,
            }
        }),
    )
    .await
    .unwrap();

    let ok = blocking_call(
        endpoint.clone(),
        request(&token, "Stop"),
        Duration::from_secs(5),
    )
    .await
    .unwrap();
    assert_eq!(ok.stdout.as_deref(), Some("seen \"Stop\""));

    // Wrong token: the server answers with an empty response, the handler is not called.
    let bad = blocking_call(
        endpoint.clone(),
        request("x".repeat(64).as_str(), "Stop"),
        Duration::from_secs(5),
    )
    .await
    .unwrap();
    assert_eq!(bad, HookResponse::default());

    // Many concurrent hook invocations.
    let calls: Vec<_> = (0..20)
        .map(|i| {
            blocking_call(
                endpoint.clone(),
                request(&token, &format!("E{i}")),
                Duration::from_secs(10),
            )
        })
        .collect();
    for (i, result) in futures_join(calls).await.into_iter().enumerate() {
        assert_eq!(result.unwrap().stdout.unwrap(), format!("seen \"E{i}\""));
    }

    // Only one server per endpoint.
    assert!(start(
        endpoint.clone(),
        token.clone(),
        handler(|_| async { HookResponse::default() })
    )
    .await
    .is_err());
    drop(server);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn client_fails_fast_when_app_is_not_running() {
    let dir = temp_dir("down");
    let endpoint = Endpoint::for_data_dir(&dir);
    let started = std::time::Instant::now();
    let err = blocking_call(endpoint, request("t", "Stop"), Duration::from_secs(5))
        .await
        .unwrap_err();
    assert!(matches!(err, ClientError::NotRunning), "{err}");
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[tokio::test(flavor = "multi_thread")]
async fn slow_handlers_time_out_on_the_client_side() {
    let dir = temp_dir("slow");
    let token = ao_ipc::token::ensure_token(&dir).unwrap();
    let endpoint = Endpoint::for_data_dir(&dir);
    let _server = start(
        endpoint.clone(),
        token.clone(),
        handler(|_| async {
            tokio::time::sleep(Duration::from_secs(5)).await;
            HookResponse::default()
        }),
    )
    .await
    .unwrap();
    let err = blocking_call(
        endpoint,
        request(&token, "PermissionRequest"),
        Duration::from_millis(300),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, ClientError::Timeout));
    let _ = std::fs::remove_dir_all(dir);
}

async fn futures_join<T>(futures: Vec<impl std::future::Future<Output = T>>) -> Vec<T> {
    let mut out = Vec::new();
    let handles: Vec<_> = futures.into_iter().map(|f| Box::pin(f)).collect();
    for h in handles {
        out.push(h.await);
    }
    out
}
