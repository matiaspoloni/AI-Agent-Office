//! Ingest pipeline: de-duplication, sanitization and project resolution.
//! Runs before an event reaches the state, the database or the UI.

use crate::event::{AgentEvent, EventKind};
use crate::ids::{session_key, ProjectId};
use crate::sanitize::{sanitize_event, SanitizeLimits};
use crate::time::now_ms;
use std::collections::{HashMap, HashSet, VecDeque};

const DEDUPE_WINDOW: usize = 4096;
/// Events stamped further in the future than this are re-stamped with "now".
const MAX_CLOCK_SKEW_MS: i64 = 5 * 60 * 1000;
/// 2000-01-01: anything older is a unit mix-up (seconds instead of ms).
const MIN_TIMESTAMP_MS: i64 = 946_684_800_000;

/// Result of running one event through the pipeline.
// Short-lived return value, moved immediately; boxing would only add an allocation.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum Ingest {
    Accepted(AgentEvent),
    Duplicate,
    /// The event broke an invariant. It is dropped and reported in Diagnostics.
    Rejected {
        provider: String,
        event_type: String,
        reason: String,
    },
}

/// A project known to Agent Office, used to map a session's cwd to a project.
#[derive(Debug, Clone)]
pub struct ProjectRoot {
    pub id: ProjectId,
    pub path: String,
}

/// Normalizes a path for prefix comparison: forward slashes, no trailing
/// slash, lowercase on Windows-style paths (NTFS is case-insensitive).
pub fn normalize_path(path: &str) -> String {
    let mut p = path.replace('\\', "/");
    while p.len() > 1 && p.ends_with('/') {
        p.pop();
    }
    let windows_like = p.len() >= 2 && p.as_bytes()[1] == b':' || path.contains('\\');
    if windows_like {
        p = p.to_lowercase();
    }
    p
}

fn is_within(child: &str, root: &str) -> bool {
    child == root
        || child
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/'))
}

#[derive(Debug, Default)]
pub struct Pipeline {
    limits: SanitizeLimits,
    seen: HashSet<String>,
    seen_order: VecDeque<String>,
    projects: Vec<(ProjectId, String)>,
    session_projects: HashMap<String, ProjectId>,
    duplicates: u64,
    rejected: u64,
}

impl Pipeline {
    pub fn new(limits: SanitizeLimits) -> Self {
        Self {
            limits,
            ..Default::default()
        }
    }

    pub fn set_limits(&mut self, limits: SanitizeLimits) {
        self.limits = limits;
    }

    pub fn set_projects(&mut self, projects: Vec<ProjectRoot>) {
        let mut normalized: Vec<_> = projects
            .into_iter()
            .map(|p| (p.id, normalize_path(&p.path)))
            .collect();
        // Longest path first so nested projects win.
        normalized.sort_by_key(|p| std::cmp::Reverse(p.1.len()));
        self.projects = normalized;
        self.session_projects.clear();
    }

    pub fn duplicates(&self) -> u64 {
        self.duplicates
    }

    pub fn rejected(&self) -> u64 {
        self.rejected
    }

    pub fn resolve_project(&self, cwd: &str) -> Option<ProjectId> {
        let cwd = normalize_path(cwd);
        self.projects
            .iter()
            .find(|(_, root)| is_within(&cwd, root))
            .map(|(id, _)| id.clone())
    }

    fn is_duplicate(&mut self, event: &AgentEvent) -> bool {
        let Some(correlation) = event.kind.correlation_id() else {
            return false;
        };
        let key = format!(
            "{}|{}|{}|{}",
            event.provider,
            event.session_id,
            event.type_name(),
            correlation
        );
        if self.seen.contains(&key) {
            self.duplicates += 1;
            return true;
        }
        self.seen.insert(key.clone());
        self.seen_order.push_back(key);
        if self.seen_order.len() > DEDUPE_WINDOW {
            if let Some(old) = self.seen_order.pop_front() {
                self.seen.remove(&old);
            }
        }
        false
    }

    /// Returns the cleaned event, or `None` if it is a duplicate or invalid.
    pub fn process(&mut self, event: AgentEvent) -> Option<AgentEvent> {
        match self.ingest(event) {
            Ingest::Accepted(event) => Some(event),
            Ingest::Duplicate | Ingest::Rejected { .. } => None,
        }
    }

    /// Validates, de-duplicates, sanitizes and enriches one event.
    pub fn ingest(&mut self, mut event: AgentEvent) -> Ingest {
        if let Err(reason) = event.validate() {
            self.rejected += 1;
            return Ingest::Rejected {
                provider: event.provider.0.clone(),
                event_type: event.type_name().to_owned(),
                reason,
            };
        }
        let now = now_ms();
        if event.timestamp > now + MAX_CLOCK_SKEW_MS || event.timestamp < MIN_TIMESTAMP_MS {
            event.timestamp = now;
        }
        if self.is_duplicate(&event) {
            return Ingest::Duplicate;
        }
        let mut event = sanitize_event(event, &self.limits);

        let skey = session_key(&event.provider, &event.session_id);
        let cwd = match &event.kind {
            EventKind::SessionStarted(info) | EventKind::SessionUpdated(info) => info.cwd.clone(),
            EventKind::CommandStarted(cmd) => cmd.cwd.clone(),
            _ => None,
        };
        if event.project_id.is_none() {
            if let Some(project) = self.session_projects.get(&skey) {
                event.project_id = Some(project.clone());
            } else if let Some(project) = cwd.as_deref().and_then(|c| self.resolve_project(c)) {
                self.session_projects.insert(skey, project.clone());
                event.project_id = Some(project);
            }
        }
        Ingest::Accepted(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::*;

    fn pipeline() -> Pipeline {
        let mut p = Pipeline::new(SanitizeLimits::default());
        p.set_projects(vec![
            ProjectRoot {
                id: "nalu".into(),
                path: "C:\\Users\\matia\\OneDrive\\Desktop\\Nalu Claude".into(),
            },
            ProjectRoot {
                id: "munder".into(),
                path: "C:\\Projects\\Munder-Difflin".into(),
            },
            ProjectRoot {
                id: "web".into(),
                path: "C:\\Projects\\Munder-Difflin\\web".into(),
            },
        ]);
        p
    }

    fn started(cwd: &str) -> AgentEvent {
        AgentEvent::for_session(
            "claude",
            "s1",
            EventSource::Hook,
            EventKind::SessionStarted(SessionInfo {
                cwd: Some(cwd.into()),
                ..Default::default()
            }),
        )
    }

    #[test]
    fn resolves_projects_case_insensitively_and_prefers_nested() {
        let p = pipeline();
        assert_eq!(
            p.resolve_project("c:/users/MATIA/onedrive/desktop/nalu claude/src")
                .unwrap()
                .0,
            "nalu"
        );
        assert_eq!(
            p.resolve_project("C:\\Projects\\Munder-Difflin\\web\\src")
                .unwrap()
                .0,
            "web"
        );
        assert_eq!(
            p.resolve_project("C:\\Projects\\Munder-Difflin").unwrap().0,
            "munder"
        );
        assert!(p
            .resolve_project("C:\\Projects\\Munder-Difflin-old")
            .is_none());
    }

    #[test]
    fn remembers_project_for_the_whole_session() {
        let mut p = pipeline();
        let first = p.process(started("C:\\Projects\\Munder-Difflin")).unwrap();
        assert_eq!(first.project_id.unwrap().0, "munder");
        let next = p
            .process(AgentEvent::for_session(
                "claude",
                "s1",
                EventSource::Hook,
                EventKind::AgentIdle(TextNote::default()),
            ))
            .unwrap();
        assert_eq!(next.project_id.unwrap().0, "munder");
    }

    #[test]
    fn drops_duplicate_tool_events() {
        let mut p = pipeline();
        let tool = AgentEvent::for_session(
            "claude",
            "s1",
            EventSource::Hook,
            EventKind::ToolStarted(ToolStarted {
                tool_call_id: Some("toolu_1".into()),
                tool_name: "Bash".into(),
                category: ToolCategory::Execute,
                title: None,
            }),
        );
        assert!(p.process(tool.clone()).is_some());
        assert!(p.process(tool).is_none());
        assert_eq!(p.duplicates(), 1);
    }

    #[test]
    fn rejects_invalid_events_and_fixes_bad_clocks() {
        let mut p = pipeline();
        let mut bad = started("C:\\x");
        bad.provider = "Not Valid".into();
        assert!(matches!(p.ingest(bad), Ingest::Rejected { .. }));
        assert_eq!(p.rejected(), 1);

        let seconds_not_ms = started("C:\\x").at(1_700_000_000);
        let Ingest::Accepted(fixed) = p.ingest(seconds_not_ms) else {
            panic!()
        };
        assert!(fixed.timestamp > MIN_TIMESTAMP_MS);
    }

    #[test]
    fn normalizes_unix_paths_without_lowercasing() {
        assert_eq!(normalize_path("/home/Me/proj/"), "/home/Me/proj");
        assert_eq!(normalize_path("C:\\A\\B\\"), "c:/a/b");
    }
}
