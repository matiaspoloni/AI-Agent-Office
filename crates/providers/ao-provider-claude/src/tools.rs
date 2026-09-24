//! Claude Code tool names → normalized categories and short titles.

use ao_core::event::ToolCategory;
use serde_json::Value;

pub fn category(tool: &str) -> ToolCategory {
    if tool.starts_with("mcp__") {
        return ToolCategory::Mcp;
    }
    match tool {
        "Read" | "NotebookRead" => ToolCategory::Read,
        "Grep" | "Glob" | "LS" | "ToolSearch" => ToolCategory::Search,
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => ToolCategory::Edit,
        "Bash" | "PowerShell" | "BashOutput" | "KillShell" | "KillBash" | "Monitor" => {
            ToolCategory::Execute
        }
        "WebFetch" | "WebSearch" => ToolCategory::Fetch,
        "Task" | "Agent" => ToolCategory::Subagent,
        _ => ToolCategory::Other,
    }
}

pub fn is_shell(tool: &str) -> bool {
    matches!(tool, "Bash" | "PowerShell")
}

fn str_field<'a>(input: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|k| input.get(*k).and_then(Value::as_str))
        .filter(|s| !s.is_empty())
}

/// File path a file tool works on.
pub fn file_path(input: &Value) -> Option<String> {
    str_field(input, &["file_path", "notebook_path", "path"]).map(str::to_owned)
}

/// Shell command of a Bash/PowerShell call.
pub fn command(input: &Value) -> Option<String> {
    str_field(input, &["command"]).map(str::to_owned)
}

/// Shows `path` relative to `cwd` when it lives inside it.
pub fn relative(path: &str, cwd: Option<&str>) -> String {
    let Some(cwd) = cwd.filter(|c| !c.is_empty()) else {
        return path.to_owned();
    };
    let norm = |p: &str| p.replace('\\', "/");
    let (p, c) = (norm(path), norm(cwd));
    let c = c.trim_end_matches('/');
    let windows = c.len() >= 2 && c.as_bytes()[1] == b':';
    let stripped = if windows {
        p.to_lowercase()
            .strip_prefix(&c.to_lowercase())
            .map(|_| p[c.len()..].to_owned())
    } else {
        p.strip_prefix(c).map(str::to_owned)
    };
    match stripped {
        Some(rest) if rest.starts_with('/') => rest.trim_start_matches('/').to_owned(),
        _ => path.to_owned(),
    }
}

fn first_line(text: &str, max: usize) -> String {
    let line = text.lines().next().unwrap_or_default().trim();
    if line.chars().count() > max {
        format!("{}…", line.chars().take(max).collect::<String>())
    } else {
        line.to_owned()
    }
}

/// Short, human readable description of a tool call ("Edit src/main.rs").
pub fn title(tool: &str, input: &Value, cwd: Option<&str>) -> String {
    let path = file_path(input).map(|p| relative(&p, cwd));
    match tool {
        "Read" | "NotebookRead" => format!("Read {}", path.unwrap_or_default()),
        "Edit" | "MultiEdit" | "NotebookEdit" => format!("Edit {}", path.unwrap_or_default()),
        "Write" => format!("Write {}", path.unwrap_or_default()),
        "Bash" | "PowerShell" => command(input)
            .map(|c| first_line(&c, 120))
            .unwrap_or_else(|| tool.to_owned()),
        "Grep" => format!(
            "Search \"{}\"",
            str_field(input, &["pattern"])
                .map(|p| first_line(p, 60))
                .unwrap_or_default()
        ),
        "Glob" => format!(
            "Find {}",
            str_field(input, &["pattern"]).unwrap_or_default()
        ),
        "WebFetch" => format!(
            "Fetch {}",
            str_field(input, &["url"])
                .map(|u| first_line(u, 80))
                .unwrap_or_default()
        ),
        "WebSearch" => format!(
            "Search the web: {}",
            str_field(input, &["query"])
                .map(|q| first_line(q, 60))
                .unwrap_or_default()
        ),
        "Task" | "Agent" => format!(
            "Delegate: {}",
            str_field(input, &["description", "subagent_type"])
                .map(|d| first_line(d, 80))
                .unwrap_or_default()
        ),
        "TodoWrite" => "Update the todo list".into(),
        "AskUserQuestion" => "Ask you a question".into(),
        other if other.starts_with("mcp__") => {
            let mut parts = other.trim_start_matches("mcp__").splitn(2, "__");
            let server = parts.next().unwrap_or_default();
            let name = parts.next().unwrap_or_default();
            format!("{server} · {name}")
        }
        other => other.to_owned(),
    }
    .trim()
    .to_owned()
}

/// What a permission request is about ("Run: npm install").
pub fn permission_description(tool: &str, input: &Value, cwd: Option<&str>) -> String {
    match tool {
        "Bash" | "PowerShell" => format!(
            "Run: {}",
            command(input)
                .map(|c| first_line(&c, 160))
                .unwrap_or_default()
        ),
        _ => title(tool, input, cwd),
    }
}

/// Parses the exit code Claude Code puts at the start of a failed Bash
/// result (`"Exit code 1\n…"`), only when it is literally there.
pub fn exit_code_from_error(error: &str) -> Option<i32> {
    error
        .strip_prefix("Exit code ")?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn categories() {
        assert_eq!(category("Read"), ToolCategory::Read);
        assert_eq!(category("Grep"), ToolCategory::Search);
        assert_eq!(category("MultiEdit"), ToolCategory::Edit);
        assert_eq!(category("PowerShell"), ToolCategory::Execute);
        assert_eq!(category("mcp__github__create_issue"), ToolCategory::Mcp);
        assert_eq!(category("Agent"), ToolCategory::Subagent);
        assert_eq!(category("SomethingNew"), ToolCategory::Other);
    }

    #[test]
    fn titles_are_short_and_relative() {
        let cwd = Some("C:\\Projects\\Nalu");
        assert_eq!(
            title(
                "Edit",
                &json!({"file_path": "C:\\Projects\\Nalu\\src\\main.rs"}),
                cwd
            ),
            "Edit src/main.rs"
        );
        assert_eq!(
            title("Read", &json!({"file_path": "/other/x.rs"}), cwd),
            "Read /other/x.rs"
        );
        assert_eq!(
            title("Bash", &json!({"command": "npm test\n--watch"}), cwd),
            "npm test"
        );
        assert_eq!(
            title("mcp__github__create_issue", &json!({}), cwd),
            "github · create_issue"
        );
        assert_eq!(
            relative("/home/u/p/src/a.ts", Some("/home/u/p/")),
            "src/a.ts"
        );
        assert_eq!(
            relative("/home/u/project2/a.ts", Some("/home/u/p")),
            "/home/u/project2/a.ts"
        );
    }

    #[test]
    fn exit_codes_only_when_present() {
        assert_eq!(exit_code_from_error("Exit code 1\nError: boom"), Some(1));
        assert_eq!(exit_code_from_error("Command timed out"), None);
    }
}
