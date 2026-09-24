//! Visual activity of an agent, derived from normalized events.

use crate::event::ToolCategory;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export)]
pub enum Activity {
    Idle,
    Thinking,
    Reading,
    Coding,
    RunningCommand,
    Testing,
    WaitingPermission,
    /// The agent asked the user something and waits for an answer.
    WaitingInput,
    Error,
    Done,
}

static TEST_COMMAND: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?ix)
        \b(npm|pnpm|yarn|bun)\s+(run\s+)?test\b
      | \b(npx\s+)?(vitest|jest|mocha|playwright\s+test|cypress\s+run)\b
      | \bcargo\s+(test|nextest)\b
      | \bpytest\b | \bpython\s+-m\s+(pytest|unittest)\b
      | \bgo\s+test\b
      | \bdotnet\s+test\b
      | \b(mvn|mvnw)\s+(\S+\s+)*test\b | \bgradlew?\s+(\S+\s+)*test\b
      | \bphpunit\b | \brspec\b | \bctest\b
      | \bInvoke-Pester\b
    ",
    )
    .expect("valid test-runner regex")
});

/// Display heuristic: does this shell command look like it runs a test suite?
/// Only used to pick the QA area in the office; never changes stored data.
pub fn looks_like_test_command(command: &str) -> bool {
    TEST_COMMAND.is_match(command)
}

/// Activity shown while a tool of this category runs.
pub fn activity_for_tool(category: ToolCategory, title: Option<&str>) -> Activity {
    match category {
        ToolCategory::Read | ToolCategory::Search | ToolCategory::Fetch => Activity::Reading,
        ToolCategory::Edit => Activity::Coding,
        ToolCategory::Execute => {
            if title.is_some_and(looks_like_test_command) {
                Activity::Testing
            } else {
                Activity::RunningCommand
            }
        }
        ToolCategory::Think | ToolCategory::Subagent | ToolCategory::Mcp | ToolCategory::Other => {
            Activity::Thinking
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_common_test_runners() {
        for cmd in [
            "npm test",
            "npm run test -- --watch=false",
            "pnpm test",
            "cargo test -p ao-core",
            "python -m pytest tests/",
            "pytest -x",
            "go test ./...",
            "dotnet test",
            "npx vitest run",
            "./gradlew clean test",
            "Invoke-Pester -Path tests",
        ] {
            assert!(looks_like_test_command(cmd), "{cmd}");
        }
    }

    #[test]
    fn ignores_non_test_commands() {
        for cmd in [
            "npm install",
            "cargo build",
            "git status",
            "ls tests",
            "cat test.txt",
            "npm run build",
        ] {
            assert!(!looks_like_test_command(cmd), "{cmd}");
        }
    }

    #[test]
    fn maps_tool_categories() {
        assert_eq!(
            activity_for_tool(ToolCategory::Edit, None),
            Activity::Coding
        );
        assert_eq!(
            activity_for_tool(ToolCategory::Search, None),
            Activity::Reading
        );
        assert_eq!(
            activity_for_tool(ToolCategory::Execute, Some("ls")),
            Activity::RunningCommand
        );
        assert_eq!(
            activity_for_tool(ToolCategory::Execute, Some("cargo test")),
            Activity::Testing
        );
    }
}
