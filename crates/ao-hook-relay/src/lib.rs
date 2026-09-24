//! The hook relay. Agent CLIs run it as an exec-form hook command:
//!
//! ```text
//! agent-office hook <provider> [--origin global|managed] [--wait <secs>] [--data-dir <dir>]
//! ```
//!
//! It reads the hook JSON from stdin, forwards it to the running app over the
//! local IPC endpoint, prints whatever the app answers and exits with the
//! app's exit code. **It is fail-open**: if the app is not running, the token
//! is missing, or anything goes wrong, it prints nothing and exits 0 within a
//! few hundred milliseconds, so the agent behaves as if Agent Office did not
//! exist.

use ao_ipc::{client, paths, token, Endpoint, HookOrigin, HookRequest, PROTOCOL_VERSION};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

/// Hook payloads larger than this are truncated (they are only observed).
pub const MAX_STDIN_BYTES: usize = 4 * 1024 * 1024;
pub const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
pub const DEFAULT_WAIT_SECS: u64 = 5;
pub const MAX_WAIT_SECS: u64 = 3600;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayArgs {
    pub provider: String,
    pub origin: HookOrigin,
    pub wait: Duration,
    pub data_dir: Option<PathBuf>,
}

/// Parses the arguments after the program name. A leading `hook` subcommand
/// is accepted (that is how the main app binary is invoked).
pub fn parse_args(args: &[String]) -> Result<RelayArgs, String> {
    let mut iter = args.iter().peekable();
    if iter.peek().is_some_and(|a| a.as_str() == "hook") {
        iter.next();
    }
    let provider = iter.next().ok_or("missing provider id")?.clone();
    if provider.is_empty()
        || !provider
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        return Err(format!("invalid provider id `{provider}`"));
    }
    let mut parsed = RelayArgs {
        provider,
        origin: HookOrigin::Global,
        wait: Duration::from_secs(DEFAULT_WAIT_SECS),
        data_dir: None,
    };
    while let Some(flag) = iter.next() {
        let value = iter.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--origin" => {
                parsed.origin = match value.as_str() {
                    "global" => HookOrigin::Global,
                    "managed" => HookOrigin::Managed,
                    other => return Err(format!("unknown origin `{other}`")),
                }
            }
            "--wait" => {
                let secs: u64 = value
                    .parse()
                    .map_err(|_| format!("invalid --wait `{value}`"))?;
                parsed.wait = Duration::from_secs(secs.clamp(1, MAX_WAIT_SECS));
            }
            "--data-dir" => parsed.data_dir = Some(PathBuf::from(value)),
            other => return Err(format!("unknown flag `{other}`")),
        }
    }
    Ok(parsed)
}

fn read_payload(stdin: &mut impl Read) -> (serde_json::Value, bool) {
    let mut buf = Vec::new();
    let _ = stdin.take(MAX_STDIN_BYTES as u64 + 1).read_to_end(&mut buf);
    let truncated = buf.len() > MAX_STDIN_BYTES;
    buf.truncate(MAX_STDIN_BYTES);
    match serde_json::from_slice::<serde_json::Value>(&buf) {
        Ok(value) => (value, truncated),
        Err(_) => (
            serde_json::json!({ "raw": String::from_utf8_lossy(&buf) }),
            truncated,
        ),
    }
}

/// Runs the relay. Returns the process exit code.
pub fn run(args: &RelayArgs, stdin: &mut impl Read, stdout: &mut impl Write) -> i32 {
    let received_at_ms = ao_ipc::now_ms();
    let (payload, payload_truncated) = read_payload(stdin);
    let data_dir = args
        .data_dir
        .clone()
        .unwrap_or_else(paths::resolve_data_dir);
    let Some(token) = token::read_token(&data_dir) else {
        return 0; // App never ran for this user: nothing to report to.
    };
    let request = HookRequest {
        v: PROTOCOL_VERSION,
        token,
        provider: args.provider.clone(),
        origin: args.origin,
        received_at_ms,
        relay_pid: std::process::id(),
        payload,
        payload_truncated,
    };
    let endpoint = Endpoint::for_data_dir(&data_dir);
    match client::call(&endpoint, &request, CONNECT_TIMEOUT, args.wait) {
        Ok(response) => {
            if let Some(text) = response.stdout {
                let _ = stdout.write_all(text.as_bytes());
                let _ = stdout.flush();
            }
            response.exit_code
        }
        Err(_) => 0,
    }
}

/// Entry point used by both binaries: parses `std::env::args`, reads stdin,
/// writes stdout. Bad arguments are ignored (fail-open) after printing a
/// message to stderr, which agent CLIs only show in debug logs.
pub fn main_from_env() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(&args) {
        Ok(parsed) => run(
            &parsed,
            &mut std::io::stdin().lock(),
            &mut std::io::stdout().lock(),
        ),
        Err(err) => {
            eprintln!("agent-office hook: {err}");
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parses_arguments() {
        let a = parse_args(&s(&[
            "hook", "claude", "--origin", "managed", "--wait", "600",
        ]))
        .unwrap();
        assert_eq!(a.provider, "claude");
        assert_eq!(a.origin, HookOrigin::Managed);
        assert_eq!(a.wait, Duration::from_secs(600));
        let b = parse_args(&s(&["codex"])).unwrap();
        assert_eq!(b.origin, HookOrigin::Global);
        assert!(parse_args(&s(&[])).is_err());
        assert!(parse_args(&s(&["Claude Code"])).is_err());
        assert!(parse_args(&s(&["claude", "--bogus", "x"])).is_err());
        assert_eq!(
            parse_args(&s(&["claude", "--wait", "999999"]))
                .unwrap()
                .wait,
            Duration::from_secs(MAX_WAIT_SECS)
        );
    }

    #[test]
    fn non_json_input_is_wrapped() {
        let (value, truncated) = read_payload(&mut "not json".as_bytes());
        assert_eq!(value["raw"], "not json");
        assert!(!truncated);
    }

    #[test]
    fn fails_open_without_app() {
        let dir = std::env::temp_dir().join(format!("ao-relay-noapp-{}", std::process::id()));
        let args = RelayArgs {
            provider: "claude".into(),
            origin: HookOrigin::Global,
            wait: Duration::from_secs(1),
            data_dir: Some(dir.clone()),
        };
        let mut out = Vec::new();
        let started = std::time::Instant::now();
        assert_eq!(
            run(
                &args,
                &mut r#"{"hook_event_name":"Stop"}"#.as_bytes(),
                &mut out
            ),
            0
        );
        assert!(out.is_empty());
        // With a token but no server it must still be quick.
        ao_ipc::token::ensure_token(&dir).unwrap();
        assert_eq!(run(&args, &mut "{}".as_bytes(), &mut out), 0);
        assert!(out.is_empty());
        assert!(started.elapsed() < Duration::from_secs(2));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn forwards_payload_and_prints_the_answer() {
        let dir = std::env::temp_dir().join(format!("ao-relay-app-{}", std::process::id()));
        let token = ao_ipc::token::ensure_token(&dir).unwrap();
        let _server = ao_ipc::server::start(
            Endpoint::for_data_dir(&dir),
            token,
            ao_ipc::server::handler(|req: HookRequest| async move {
                assert_eq!(req.provider, "claude");
                ao_ipc::HookResponse {
                    stdout: Some(format!("{{\"event\":{}}}", req.payload["hook_event_name"])),
                    exit_code: 0,
                }
            }),
        )
        .await
        .unwrap();
        let args = RelayArgs {
            provider: "claude".into(),
            origin: HookOrigin::Managed,
            wait: Duration::from_secs(5),
            data_dir: Some(dir.clone()),
        };
        let out = tokio::task::spawn_blocking(move || {
            let mut out = Vec::new();
            let code = run(
                &args,
                &mut r#"{"hook_event_name":"PermissionRequest"}"#.as_bytes(),
                &mut out,
            );
            (code, String::from_utf8(out).unwrap())
        })
        .await
        .unwrap();
        assert_eq!(out, (0, "{\"event\":\"PermissionRequest\"}".to_string()));
        let _ = std::fs::remove_dir_all(dir);
    }
}
