//! Secret redaction and size limits applied to every event before it is
//! stored, logged or sent to the UI.

use crate::event::AgentEvent;
use once_cell::sync::Lazy;
use regex::{Captures, Regex};
use serde_json::Value;

pub const REDACTED: &str = "[REDACTED]";

#[derive(Debug, Clone, Copy)]
pub struct SanitizeLimits {
    /// Maximum characters kept per string field (command output, tool detail…).
    pub max_string_chars: usize,
    /// Keep prompt text. When false, `prompt.submitted` text is dropped.
    pub store_prompts: bool,
}

impl Default for SanitizeLimits {
    fn default() -> Self {
        Self {
            max_string_chars: 8 * 1024,
            store_prompts: true,
        }
    }
}

static TOKEN_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    [
        // Private key blocks.
        r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
        // Anthropic / OpenAI style keys.
        r"\bsk-(?:ant-|proj-|live-)?[A-Za-z0-9_\-]{16,}",
        // GitHub tokens.
        r"\bgh[pousr]_[A-Za-z0-9]{20,}",
        r"\bgithub_pat_[A-Za-z0-9_]{20,}",
        // AWS access key ids.
        r"\bAKIA[0-9A-Z]{16}\b",
        // Slack tokens.
        r"\bxox[abprs]-[A-Za-z0-9-]{10,}",
        // Google API keys.
        r"\bAIza[0-9A-Za-z_\-]{30,}",
        // Bearer tokens.
        r"(?i)\bbearer\s+[A-Za-z0-9\-._~+/]{12,}=*",
    ]
    .iter()
    .map(|p| Regex::new(p).expect("valid redaction regex"))
    .collect()
});

/// `NAME_API_KEY=value`, `token: value`, `"password": "value"` …
static ASSIGNMENT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?i)(?P<name>[A-Za-z0-9_\-]*(?:api[_\-]?key|secret|token|password|passwd|authorization)[A-Za-z0-9_\-]*)(?P<sep>["']?\s*[:=]\s*["']?)(?P<value>[^\s"',;]{6,})"#,
    )
    .expect("valid assignment regex")
});

/// Masks anything that looks like a credential.
pub fn redact(text: &str) -> String {
    let mut out = text.to_owned();
    for pattern in TOKEN_PATTERNS.iter() {
        if pattern.is_match(&out) {
            out = pattern.replace_all(&out, REDACTED).into_owned();
        }
    }
    if ASSIGNMENT.is_match(&out) {
        out = ASSIGNMENT
            .replace_all(&out, |c: &Captures| {
                if &c["value"] == REDACTED {
                    c[0].to_owned()
                } else {
                    format!("{}{}{}", &c["name"], &c["sep"], REDACTED)
                }
            })
            .into_owned();
    }
    out
}

fn truncate(text: String, max: usize) -> String {
    if text.chars().count() <= max {
        return text;
    }
    let mut kept: String = text.chars().take(max).collect();
    kept.push_str("… [truncated]");
    kept
}

fn sanitize_value(value: &mut Value, limits: &SanitizeLimits) {
    match value {
        Value::String(s) => {
            let cleaned = truncate(redact(s), limits.max_string_chars);
            *s = cleaned;
        }
        Value::Array(items) => items.iter_mut().for_each(|v| sanitize_value(v, limits)),
        Value::Object(map) => map.values_mut().for_each(|v| sanitize_value(v, limits)),
        _ => {}
    }
}

/// Redacts and truncates every string inside the event payload.
/// Envelope ids are left untouched.
pub fn sanitize_event(event: AgentEvent, limits: &SanitizeLimits) -> AgentEvent {
    let mut json = match serde_json::to_value(&event) {
        Ok(v) => v,
        Err(_) => return event,
    };
    if let Some(payload) = json.get_mut("payload") {
        if !limits.store_prompts && event.type_name() == "prompt.submitted" {
            if let Some(obj) = payload.as_object_mut() {
                obj.remove("text");
            }
        }
        sanitize_value(payload, limits);
    }
    serde_json::from_value(json).unwrap_or(event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::*;

    #[test]
    fn redacts_common_secrets() {
        let input = "export ANTHROPIC_API_KEY=sk-ant-api03-abcdefghijklmnopqrstuvwxyz and ghp_abcdefghijklmnopqrstuvwx1234";
        let out = redact(input);
        assert!(!out.contains("sk-ant-api03"), "{out}");
        assert!(!out.contains("ghp_abc"), "{out}");
        assert!(out.contains("ANTHROPIC_API_KEY="));

        let out = redact(r#"{"password": "hunter2hunter2"}"#);
        assert!(!out.contains("hunter2"), "{out}");

        let out = redact("Authorization: Bearer abcdefghijklmnop.qrstu");
        assert!(!out.contains("abcdefghijklmnop"), "{out}");
    }

    #[test]
    fn keeps_normal_text() {
        let text = "cargo test -p ao-core && git status";
        assert_eq!(redact(text), text);
        assert_eq!(redact("token count: 42"), "token count: 42");
    }

    #[test]
    fn sanitizes_payload_and_truncates() {
        let event = AgentEvent::for_session(
            "codex",
            "t1",
            EventSource::Protocol,
            EventKind::CommandOutput(CommandOutput {
                command_id: None,
                stream: OutputStream::Stdout,
                chunk: format!(
                    "OPENAI_API_KEY=sk-proj-{} {}",
                    "a".repeat(30),
                    "x".repeat(100)
                ),
            }),
        );
        let limits = SanitizeLimits {
            max_string_chars: 60,
            store_prompts: true,
        };
        let clean = sanitize_event(event.clone(), &limits);
        let EventKind::CommandOutput(out) = &clean.kind else {
            panic!()
        };
        assert!(!out.chunk.contains("sk-proj"));
        assert!(out.chunk.ends_with("[truncated]"));
        assert_eq!(clean.event_id, event.event_id);
    }

    #[test]
    fn drops_prompt_text_when_disabled() {
        let event = AgentEvent::for_session(
            "claude",
            "s",
            EventSource::Hook,
            EventKind::PromptSubmitted(PromptSubmitted {
                text: Some("secret plan".into()),
            }),
        );
        let clean = sanitize_event(
            event,
            &SanitizeLimits {
                store_prompts: false,
                ..Default::default()
            },
        );
        let EventKind::PromptSubmitted(p) = clean.kind else {
            panic!()
        };
        assert!(p.text.is_none());
    }
}
