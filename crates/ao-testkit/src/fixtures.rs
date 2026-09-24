//! Fixture format (one JSON file per case):
//!
//! ```json
//! {
//!   "description": "PreToolUse for Bash starts a tool and a command",
//!   "input": { ... provider payload ... },
//!   "context": { ... optional adapter-specific options ... },
//!   "expected": [ { "type": "tool.started", "payload": { "toolName": "Bash" } }, ... ]
//! }
//! ```
//!
//! `expected` lists the produced events in order. Each expected object is a
//! *subset* of the actual event JSON: only the keys written in the fixture are
//! compared, so fixtures stay readable and robust to new optional fields.

use ao_core::AgentEvent;
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Fixture {
    pub path: PathBuf,
    pub description: String,
    pub input: Value,
    pub context: Value,
    pub expected: Vec<Value>,
}

/// Repository root (`fixtures/` lives there).
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Loads every `*.json` fixture in `fixtures/<dir>` (sorted by name).
pub fn load_dir(dir: &str) -> Vec<Fixture> {
    let root = repo_root().join("fixtures").join(dir);
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", root.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    paths.sort();
    paths.into_iter().map(|p| load(&p)).collect()
}

pub fn load(path: &Path) -> Fixture {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let value: Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{} is not JSON: {e}", path.display()));
    Fixture {
        path: path.to_path_buf(),
        description: value["description"].as_str().unwrap_or_default().to_owned(),
        input: value["input"].clone(),
        context: value.get("context").cloned().unwrap_or(Value::Null),
        expected: value["expected"]
            .as_array()
            .unwrap_or_else(|| panic!("{}: `expected` must be an array", path.display()))
            .clone(),
    }
}

/// Checks that every key in `expected` exists in `actual` with an equal value
/// (recursively for objects; arrays must match element-wise and in length).
pub fn json_subset(expected: &Value, actual: &Value, at: &str) -> Result<(), String> {
    match (expected, actual) {
        (Value::Object(exp), Value::Object(act)) => {
            for (key, value) in exp {
                let path = format!("{at}.{key}");
                match act.get(key) {
                    Some(actual_value) => json_subset(value, actual_value, &path)?,
                    None if value.is_null() => {}
                    None => return Err(format!("{path}: missing (expected {value})")),
                }
            }
            Ok(())
        }
        (Value::Array(exp), Value::Array(act)) => {
            if exp.len() != act.len() {
                return Err(format!(
                    "{at}: expected {} items, got {}",
                    exp.len(),
                    act.len()
                ));
            }
            for (i, (e, a)) in exp.iter().zip(act).enumerate() {
                json_subset(e, a, &format!("{at}[{i}]"))?;
            }
            Ok(())
        }
        _ if expected == actual => Ok(()),
        _ => Err(format!("{at}: expected {expected}, got {actual}")),
    }
}

/// Asserts the produced events match the fixture's expectations, with a
/// readable diff on failure.
pub fn assert_events(fixture: &Fixture, actual: &[AgentEvent]) {
    let actual_json: Vec<Value> = actual
        .iter()
        .map(|e| serde_json::to_value(e).expect("serialize event"))
        .collect();
    let summary = || {
        actual_json
            .iter()
            .map(|e| format!("  {} {}", e["type"], e["payload"]))
            .collect::<Vec<_>>()
            .join("\n")
    };
    if actual_json.len() != fixture.expected.len() {
        panic!(
            "{} ({}): expected {} events, got {}:\n{}",
            fixture.path.display(),
            fixture.description,
            fixture.expected.len(),
            actual_json.len(),
            summary()
        );
    }
    for (i, (expected, actual)) in fixture.expected.iter().zip(&actual_json).enumerate() {
        if let Err(diff) = json_subset(expected, actual, &format!("event[{i}]")) {
            panic!(
                "{} ({}): {diff}\nactual events:\n{}",
                fixture.path.display(),
                fixture.description,
                summary()
            );
        }
    }
    for event in actual {
        if let Err(reason) = event.validate() {
            panic!(
                "{}: produced an invalid event: {reason}",
                fixture.path.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn subset_matching() {
        let actual = json!({"type": "tool.started", "payload": {"toolName": "Bash", "category": "execute"}, "x": 1});
        assert!(json_subset(&json!({"payload": {"toolName": "Bash"}}), &actual, "e").is_ok());
        assert!(json_subset(&json!({"payload": {"toolName": "Read"}}), &actual, "e").is_err());
        assert!(json_subset(&json!({"missing": 1}), &actual, "e").is_err());
        assert!(json_subset(&json!({"missing": null}), &actual, "e").is_ok());
        assert!(json_subset(&json!([1, 2]), &json!([1]), "e").is_err());
    }
}
