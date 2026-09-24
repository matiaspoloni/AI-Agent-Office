//! `claude -p --output-format stream-json --verbose` lines → normalized events.
//!
//! In managed sessions hooks remain the source of tool/prompt/subagent
//! events (they carry ids and arrive for every agent); the stream adds what
//! only it has: the active model, API retries, assistant text, errors and
//! token usage / cost.

use crate::PROVIDER_ID;
use ao_core::event::*;
use ao_core::ids::SessionId;
use serde_json::Value;

fn s<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).filter(|x| !x.is_empty())
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        format!("{}…", text.chars().take(max).collect::<String>())
    }
}

/// Token totals from `result.modelUsage` (running totals for the whole
/// session, including subagents — `result.usage` is per turn only).
pub fn usage_from_result(result: &Value) -> Option<UsageSnapshot> {
    let models = result.get("modelUsage").and_then(Value::as_object)?;
    let mut usage = UsageSnapshot::default();
    let mut any = false;
    let add = |slot: &mut Option<u64>, v: Option<u64>| {
        if let Some(v) = v {
            *slot = Some(slot.unwrap_or(0) + v);
        }
    };
    for model in models.values() {
        let get = |k: &str| model.get(k).and_then(Value::as_u64);
        add(&mut usage.input_tokens, get("inputTokens"));
        add(&mut usage.output_tokens, get("outputTokens"));
        add(&mut usage.cached_input_tokens, get("cacheReadInputTokens"));
        add(&mut usage.reasoning_tokens, get("thinkingTokens"));
        if let Some(window) = get("contextWindow") {
            usage.context_window = Some(usage.context_window.unwrap_or(0).max(window));
        }
        any = true;
    }
    if !any {
        return None;
    }
    usage.total_tokens = match (usage.input_tokens, usage.output_tokens) {
        (None, None) => None,
        (i, o) => Some(i.unwrap_or(0) + o.unwrap_or(0)),
    };
    if let Some(cost) = result.get("total_cost_usd").and_then(Value::as_f64) {
        usage.cost_usd = Some(cost);
        // Documented as a client-side estimate.
        usage.cost_is_estimate = true;
    }
    Some(usage)
}

/// Maps one stdout line. Non-JSON lines and unknown message types are ignored.
pub fn map_stream_line(line: &str, session_id: &str, now_ms: i64) -> Vec<AgentEvent> {
    let Ok(msg) = serde_json::from_str::<Value>(line.trim()) else {
        return Vec::new();
    };
    let ev = |kind| {
        AgentEvent::for_session(
            PROVIDER_ID,
            SessionId::new(session_id),
            EventSource::Protocol,
            kind,
        )
        .at(now_ms)
    };
    match (s(&msg, "type"), s(&msg, "subtype")) {
        (Some("system"), Some("init")) => vec![ev(EventKind::SessionUpdated(SessionInfo {
            model: s(&msg, "model").map(str::to_owned),
            cwd: s(&msg, "cwd").map(str::to_owned),
            permission_mode: s(&msg, "permissionMode").map(str::to_owned),
            ..Default::default()
        }))],
        (Some("system"), Some("api_retry")) => {
            let attempt = msg.get("attempt").and_then(Value::as_u64).unwrap_or(0);
            let max = msg.get("max_retries").and_then(Value::as_u64).unwrap_or(0);
            let error = s(&msg, "error").unwrap_or("unknown");
            vec![ev(EventKind::AgentError(AgentError {
                message: format!("API error ({error}), retrying {attempt}/{max}"),
                error_type: Some(error.to_owned()),
                recoverable: true,
            }))]
        }
        (Some("assistant"), _) => {
            // Subagent traffic carries parent_tool_use_id; hooks already cover it.
            if msg.get("parent_tool_use_id").is_some_and(|p| !p.is_null()) {
                return Vec::new();
            }
            let mut out = Vec::new();
            if let Some(error) = s(&msg, "error") {
                out.push(ev(EventKind::AgentError(AgentError {
                    message: format!("API error: {error}"),
                    error_type: Some(error.to_owned()),
                    recoverable: matches!(error, "rate_limit" | "overloaded" | "server_error"),
                })));
            }
            let blocks = msg
                .pointer("/message/content")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let text: Vec<&str> = blocks
                .iter()
                .filter(|b| s(b, "type") == Some("text"))
                .filter_map(|b| s(b, "text"))
                .collect();
            if !text.is_empty() {
                out.push(ev(EventKind::AgentMessage(TextNote {
                    text: Some(clip(&text.join("\n"), 2000)),
                })));
            }
            out
        }
        (Some("result"), subtype) => {
            let mut out = Vec::new();
            if let Some(usage) = usage_from_result(&msg) {
                out.push(ev(EventKind::UsageUpdated(usage)));
            }
            let is_error = msg
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if is_error || subtype.is_some_and(|st| st.starts_with("error")) {
                let errors: Vec<String> = msg
                    .get("errors")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|e| e.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
                let message = if !errors.is_empty() {
                    errors.join("; ")
                } else {
                    s(&msg, "result")
                        .map(|r| clip(r, 300))
                        .unwrap_or_else(|| subtype.unwrap_or("error").to_owned())
                };
                out.push(ev(EventKind::AgentError(AgentError {
                    message,
                    error_type: subtype.map(str::to_owned),
                    recoverable: true,
                })));
            } else {
                out.push(ev(EventKind::AgentIdle(TextNote {
                    text: Some("Turn finished — waiting for your next prompt".into()),
                })));
            }
            out
        }
        _ => Vec::new(),
    }
}

/// The stdin line that submits a user prompt in `--input-format stream-json`.
pub fn user_message_line(prompt: &str) -> String {
    serde_json::json!({
        "type": "user",
        "message": { "role": "user", "content": prompt },
        "parent_tool_use_id": null
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn usage_sums_models_and_marks_cost_as_estimate() {
        let result = json!({
            "type": "result", "subtype": "success", "is_error": false, "total_cost_usd": 0.42,
            "modelUsage": {
                "claude-opus-5-5": {"inputTokens": 100, "outputTokens": 20, "cacheReadInputTokens": 50, "cacheCreationInputTokens": 0, "webSearchRequests": 0, "costUSD": 0.4, "contextWindow": 1000000, "maxOutputTokens": 64000},
                "claude-haiku-4-5": {"inputTokens": 10, "outputTokens": 5, "cacheReadInputTokens": 0, "cacheCreationInputTokens": 0, "webSearchRequests": 0, "costUSD": 0.02, "contextWindow": 200000, "maxOutputTokens": 8000}
            }
        });
        let usage = usage_from_result(&result).unwrap();
        assert_eq!(usage.input_tokens, Some(110));
        assert_eq!(usage.output_tokens, Some(25));
        assert_eq!(usage.cached_input_tokens, Some(50));
        assert_eq!(usage.total_tokens, Some(135));
        assert_eq!(usage.context_window, Some(1_000_000));
        assert_eq!(usage.cost_usd, Some(0.42));
        assert!(usage.cost_is_estimate);
        assert!(usage_from_result(&json!({"type": "result"})).is_none());
    }

    #[test]
    fn user_message_is_documented_shape() {
        let v: Value = serde_json::from_str(&user_message_line("hi \"there\"")).unwrap();
        assert_eq!(v["type"], "user");
        assert_eq!(v["message"]["role"], "user");
        assert_eq!(v["message"]["content"], "hi \"there\"");
    }

    #[test]
    fn garbage_is_ignored() {
        assert!(map_stream_line("not json", "s", 1).is_empty());
        assert!(map_stream_line(r#"{"type":"stream_event"}"#, "s", 1).is_empty());
    }
}
