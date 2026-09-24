//! Codex tool names → normalized categories and short titles.
//!
//! Hook payloads use Codex's hook-facing tool names (`Bash`, `apply_patch`,
//! `spawn_agent`, `mcp__<server>__<tool>`, …, see `core/src/tools/hook_names.rs`
//! in codex 0.156.1); the app-server protocol uses typed thread items instead
//! (`commandExecution`, `fileChange`, …).

use ao_core::event::ToolCategory;
use serde_json::Value;

/// Category of a tool name as it appears in hook payloads.
pub fn category(tool: &str) -> ToolCategory {
    if tool.starts_with("mcp__") {
        return ToolCategory::Mcp;
    }
    match tool {
        "Bash" | "write_stdin" => ToolCategory::Execute,
        "apply_patch" => ToolCategory::Edit,
        "spawn_agent" | "send_input" | "resume_agent" | "wait" | "close_agent" | "send_message"
        | "followup_task" | "interrupt_agent" | "list_agents" => ToolCategory::Subagent,
        "view_image" => ToolCategory::Read,
        "web_search" => ToolCategory::Fetch,
        "update_plan" => ToolCategory::Think,
        _ => ToolCategory::Other,
    }
}

pub fn is_shell(tool: &str) -> bool {
    tool == "Bash"
}

fn str_field<'a>(input: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|k| input.get(*k).and_then(Value::as_str))
        .filter(|s| !s.is_empty())
}

/// Shell command of a `Bash` call, or the patch text of an `apply_patch` call.
pub fn command(input: &Value) -> Option<String> {
    str_field(input, &["command"]).map(str::to_owned)
}

pub fn first_line(text: &str, max: usize) -> String {
    let line = text.lines().next().unwrap_or_default().trim();
    if line.chars().count() > max {
        format!("{}…", line.chars().take(max).collect::<String>())
    } else {
        line.to_owned()
    }
}

/// Shows `path` relative to `cwd` when it lives inside it (Windows aware).
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

/// What an `apply_patch` envelope does to a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchOp {
    Add,
    Update,
    Delete,
}

impl PatchOp {
    fn verb(self) -> &'static str {
        match self {
            PatchOp::Add => "add",
            PatchOp::Update => "update",
            PatchOp::Delete => "delete",
        }
    }
}

/// Files named in an `apply_patch` envelope. Only the documented header
/// lines are read (`*** Add File: p`, `*** Update File: p`,
/// `*** Delete File: p`, `*** Move to: p`); nothing else is interpreted.
pub fn patch_files(patch: &str) -> Vec<(PatchOp, String)> {
    let mut files: Vec<(PatchOp, String)> = Vec::new();
    for line in patch.lines() {
        let line = line.trim_end();
        let (op, path) = if let Some(p) = line.strip_prefix("*** Add File: ") {
            (PatchOp::Add, p)
        } else if let Some(p) = line.strip_prefix("*** Update File: ") {
            (PatchOp::Update, p)
        } else if let Some(p) = line.strip_prefix("*** Delete File: ") {
            (PatchOp::Delete, p)
        } else if let Some(p) = line.strip_prefix("*** Move to: ") {
            // The file being updated is renamed: report the new name.
            if let Some(last) = files.last_mut() {
                last.1 = p.trim().to_owned();
            }
            continue;
        } else {
            continue;
        };
        let path = path.trim();
        if !path.is_empty() {
            files.push((op, path.to_owned()));
        }
    }
    files
}

/// "add hello.txt", "update a.rs, b.rs", "update a.rs and 3 more".
pub fn describe_changes(files: &[(PatchOp, String)]) -> String {
    match files {
        [] => "files".into(),
        [(op, path)] => format!("{} {path}", op.verb()),
        [(op, a), (op2, b)] if op == op2 => format!("{} {a}, {b}", op.verb()),
        [(op, a), rest @ ..] => format!("{} {a} and {} more", op.verb(), rest.len()),
    }
}

/// Short, human readable description of a tool call.
pub fn title(tool: &str, input: &Value) -> String {
    match tool {
        "Bash" => command(input)
            .map(|c| first_line(&c, 120))
            .unwrap_or_else(|| tool.to_owned()),
        "apply_patch" => {
            let files = command(input).map(|p| patch_files(&p)).unwrap_or_default();
            format!("Edit: {}", describe_changes(&files))
        }
        "spawn_agent" => format!(
            "Spawn agent: {}",
            str_field(input, &["message", "task_name"])
                .map(|m| first_line(m, 80))
                .unwrap_or_default()
        ),
        "write_stdin" => "Send input to a running command".into(),
        "view_image" => "View an image".into(),
        "update_plan" => "Update the plan".into(),
        other if other.starts_with("mcp__") => mcp_title(other),
        other => other.to_owned(),
    }
    .trim()
    .to_owned()
}

/// `mcp__memory__create_entities` → `memory · create_entities`.
pub fn mcp_title(name: &str) -> String {
    let mut parts = name.trim_start_matches("mcp__").splitn(2, "__");
    let server = parts.next().unwrap_or_default();
    let tool = parts.next().unwrap_or_default();
    format!("{server} · {tool}")
}

/// What a permission request is about ("Run: npm install").
pub fn permission_description(tool: &str, input: &Value) -> String {
    match tool {
        "Bash" => format!(
            "Run: {}",
            command(input)
                .map(|c| first_line(&c, 160))
                .unwrap_or_default()
        ),
        "apply_patch" => {
            let files = command(input).map(|p| patch_files(&p)).unwrap_or_default();
            format!("Apply patch: {}", describe_changes(&files))
        }
        _ => title(tool, input),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn patch_headers_are_the_only_thing_read() {
        let patch = "*** Begin Patch\n*** Add File: hello.txt\n+*** Delete File: not-a-header.txt\n*** Update File: src/a.rs\n*** Move to: src/b.rs\n@@\n-x\n+y\n*** Delete File: old.txt\n*** End Patch";
        assert_eq!(
            patch_files(patch),
            vec![
                (PatchOp::Add, "hello.txt".into()),
                (PatchOp::Update, "src/b.rs".into()),
                (PatchOp::Delete, "old.txt".into()),
            ]
        );
        assert_eq!(
            describe_changes(&patch_files(patch)),
            "add hello.txt and 2 more"
        );
        assert_eq!(describe_changes(&[]), "files");
    }

    #[test]
    fn titles_and_categories() {
        assert_eq!(category("Bash"), ToolCategory::Execute);
        assert_eq!(category("apply_patch"), ToolCategory::Edit);
        assert_eq!(category("spawn_agent"), ToolCategory::Subagent);
        assert_eq!(category("mcp__fs__read"), ToolCategory::Mcp);
        assert_eq!(category("something_new"), ToolCategory::Other);
        assert_eq!(title("Bash", &json!({"command": "ls\nmore"})), "ls");
        assert_eq!(
            mcp_title("mcp__memory__create_entities"),
            "memory · create_entities"
        );
        assert_eq!(
            relative("/home/u/p/src/a.rs", Some("/home/u/p")),
            "src/a.rs"
        );
        assert_eq!(
            relative("C:\\Work\\App\\x.rs", Some("c:\\work\\app")),
            "x.rs"
        );
        assert_eq!(relative("/elsewhere/x", Some("/home/u/p")), "/elsewhere/x");
        assert_eq!(
            permission_description(
                "apply_patch",
                &json!({"command": "*** Begin Patch\n*** Add File: a\n+x\n*** End Patch"})
            ),
            "Apply patch: add a"
        );
    }
}
