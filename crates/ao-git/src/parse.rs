//! Parsers for Git's machine-readable output. Pure functions: tested with
//! recorded output here and against real repositories in `tests/`.

use crate::model::{
    ChangeKind, CommitInfo, FileChange, StatusCounts, WorktreeInfo, WorktreeStatus,
};

/// Changed files kept per status (counts stay exact).
pub const MAX_FILES: usize = 1000;

/// Field separator and record terminator used in `--format` strings.
pub const FIELD: char = '\u{1f}';
pub const RECORD: char = '\u{1e}';

/// The `--format` that `parse_log` reads.
pub const LOG_FORMAT: &str = "--format=%H%x1f%h%x1f%an%x1f%ct%x1f%s%x1e";

fn kind(code: char) -> Option<ChangeKind> {
    match code {
        'M' => Some(ChangeKind::Modified),
        'T' => Some(ChangeKind::TypeChanged),
        'A' => Some(ChangeKind::Added),
        'D' => Some(ChangeKind::Deleted),
        'R' => Some(ChangeKind::Renamed),
        'C' => Some(ChangeKind::Copied),
        'U' => Some(ChangeKind::Conflicted),
        _ => None, // '.' = unchanged
    }
}

/// Parses `git status --porcelain=v2 --branch -z`.
///
/// Entries are NUL-terminated; a rename (`2 …`) is followed by one more
/// NUL-terminated field, its original path. Paths are taken verbatim (`-z`
/// turns off quoting), so spaces and non-ASCII names are kept.
pub fn parse_status_v2(output: &str) -> WorktreeStatus {
    let mut status = WorktreeStatus::default();
    let mut counts = StatusCounts::default();
    let mut fields = output.split('\0');
    while let Some(entry) = fields.next() {
        if entry.is_empty() {
            continue;
        }
        let mut file: Option<FileChange> = None;
        if let Some(header) = entry.strip_prefix("# ") {
            let (key, value) = header.split_once(' ').unwrap_or((header, ""));
            match key {
                "branch.oid" if value != "(initial)" => status.head = Some(value.to_owned()),
                "branch.head" if value == "(detached)" => status.detached = true,
                "branch.head" => status.branch = Some(value.to_owned()),
                "branch.upstream" => status.upstream = Some(value.to_owned()),
                "branch.ab" => {
                    for part in value.split_whitespace() {
                        if let Some(n) = part.strip_prefix('+') {
                            status.ahead = n.parse().ok();
                        } else if let Some(n) = part.strip_prefix('-') {
                            status.behind = n.parse().ok();
                        }
                    }
                }
                _ => {}
            }
        } else if let Some(rest) = entry.strip_prefix("1 ") {
            // XY sub mH mI mW hH hI path
            let parts: Vec<&str> = rest.splitn(8, ' ').collect();
            if let [xy, _, _, _, _, _, _, path] = parts[..] {
                file = Some(changed(xy, path, None, &mut counts));
            }
        } else if let Some(rest) = entry.strip_prefix("2 ") {
            // XY sub mH mI mW hH hI Xscore path, then NUL origPath
            let parts: Vec<&str> = rest.splitn(9, ' ').collect();
            let orig = fields.next().map(str::to_owned);
            if let [xy, _, _, _, _, _, _, _, path] = parts[..] {
                file = Some(changed(xy, path, orig, &mut counts));
            }
        } else if let Some(rest) = entry.strip_prefix("u ") {
            // XY sub m1 m2 m3 mW h1 h2 h3 path
            if let Some(path) = rest.splitn(10, ' ').nth(9) {
                counts.conflicted += 1;
                file = Some(FileChange {
                    path: path.to_owned(),
                    orig_path: None,
                    staged: None,
                    unstaged: Some(ChangeKind::Conflicted),
                });
            }
        } else if let Some(path) = entry.strip_prefix("? ") {
            counts.untracked += 1;
            file = Some(FileChange {
                path: path.to_owned(),
                orig_path: None,
                staged: None,
                unstaged: Some(ChangeKind::Untracked),
            });
        }
        // "! path" (ignored) is not requested and skipped if present.
        if let Some(file) = file {
            if status.files.len() < MAX_FILES {
                status.files.push(file);
            } else {
                status.files_truncated = true;
            }
        }
    }
    status.counts = counts;
    status
}

fn changed(xy: &str, path: &str, orig: Option<String>, counts: &mut StatusCounts) -> FileChange {
    let mut codes = xy.chars();
    let staged = codes.next().and_then(kind);
    let unstaged = codes.next().and_then(kind);
    if staged.is_some() {
        counts.staged += 1;
    }
    if unstaged.is_some() {
        counts.unstaged += 1;
    }
    FileChange {
        path: path.to_owned(),
        orig_path: orig,
        staged,
        unstaged,
    }
}

/// Parses `git log` output written with [`LOG_FORMAT`].
pub fn parse_log(output: &str) -> Vec<CommitInfo> {
    output
        .split(RECORD)
        .filter_map(|record| {
            let record = record.trim_start_matches(['\n', '\r']);
            let mut f = record.splitn(5, FIELD);
            let sha = f.next()?.trim();
            if sha.is_empty() {
                return None;
            }
            let short_sha = f.next()?.to_owned();
            let author = f.next()?.to_owned();
            let time = f.next()?.trim().parse::<i64>().ok()? * 1000;
            let summary = f.next().unwrap_or_default().trim_end().to_owned();
            Some(CommitInfo {
                sha: sha.to_owned(),
                short_sha,
                author,
                time,
                summary,
            })
        })
        .collect()
}

/// Parses `git worktree list --porcelain` (blank-line separated records).
pub fn parse_worktrees(output: &str) -> Vec<WorktreeInfo> {
    let mut list = Vec::new();
    let mut current: Option<WorktreeInfo> = None;
    for line in output.lines().map(|l| l.trim_end_matches('\r')) {
        if line.is_empty() {
            list.extend(current.take());
            continue;
        }
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        if key == "worktree" {
            list.extend(current.take());
            current = Some(WorktreeInfo {
                path: value.to_owned(),
                ..Default::default()
            });
            continue;
        }
        let Some(tree) = current.as_mut() else {
            continue;
        };
        match key {
            "HEAD" => tree.head = Some(value.to_owned()),
            "branch" => {
                tree.branch = Some(
                    value
                        .strip_prefix("refs/heads/")
                        .unwrap_or(value)
                        .to_owned(),
                )
            }
            "detached" => tree.detached = true,
            "bare" => tree.bare = true,
            "locked" => tree.locked = true,
            "prunable" => tree.prunable = true,
            _ => {}
        }
    }
    list.extend(current);
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_headers_and_every_entry_type() {
        let output = [
            "# branch.oid 2d0f3c8a9b1e4f5a6b7c8d9e0f1a2b3c4d5e6f7a",
            "# branch.head feature/login",
            "# branch.upstream origin/feature/login",
            "# branch.ab +3 -1",
            "1 .M N... 100644 100644 100644 aaa aaa src/main file.rs",
            "1 A. N... 000000 100644 100644 000 bbb new.txt",
            "1 MM N... 100644 100644 100644 ccc ddd both.rs",
            "2 R. N... 100644 100644 100644 eee eee R100 docs/new name.md",
            "docs/old name.md",
            "u UU N... 100644 100644 100644 100644 f1 f2 f3 conflict.rs",
            "? notes/ñandú.txt",
            "",
        ]
        .join("\0");
        let status = parse_status_v2(&output);
        assert_eq!(status.branch.as_deref(), Some("feature/login"));
        assert!(!status.detached);
        assert_eq!(
            status.head.as_deref(),
            Some("2d0f3c8a9b1e4f5a6b7c8d9e0f1a2b3c4d5e6f7a")
        );
        assert_eq!(status.upstream.as_deref(), Some("origin/feature/login"));
        assert_eq!((status.ahead, status.behind), (Some(3), Some(1)));
        assert_eq!(
            status.counts,
            StatusCounts {
                staged: 3,
                unstaged: 2,
                untracked: 1,
                conflicted: 1
            }
        );
        assert_eq!(status.counts.dirty(), 4);
        let paths: Vec<&str> = status.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "src/main file.rs",
                "new.txt",
                "both.rs",
                "docs/new name.md",
                "conflict.rs",
                "notes/ñandú.txt"
            ]
        );
        let renamed = &status.files[3];
        assert_eq!(renamed.orig_path.as_deref(), Some("docs/old name.md"));
        assert_eq!(renamed.staged, Some(ChangeKind::Renamed));
        assert_eq!(renamed.unstaged, None);
        assert_eq!(status.files[2].staged, Some(ChangeKind::Modified));
        assert_eq!(status.files[2].unstaged, Some(ChangeKind::Modified));
    }

    #[test]
    fn detached_new_repository_and_no_upstream() {
        let status = parse_status_v2("# branch.oid (initial)\0# branch.head main\0");
        assert_eq!(status.head, None);
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert_eq!((status.ahead, status.behind), (None, None));
        let status = parse_status_v2("# branch.oid abc\0# branch.head (detached)\0");
        assert!(status.detached);
        assert_eq!(status.branch, None);
        assert_eq!(parse_status_v2(""), WorktreeStatus::default());
    }

    #[test]
    fn long_file_lists_are_capped_but_counted() {
        let output: String = (0..MAX_FILES + 5).map(|i| format!("? f{i}\0")).collect();
        let status = parse_status_v2(&output);
        assert_eq!(status.files.len(), MAX_FILES);
        assert!(status.files_truncated);
        assert_eq!(status.counts.untracked as usize, MAX_FILES + 5);
    }

    #[test]
    fn log_records_keep_separators_out_of_the_summary() {
        let output = format!(
            "a1{f}a1s{f}Ana López{f}1700000000{f}Fix: login; add tests{r}\nb2{f}b2s{f}Bot{f}1700000100{f}{r}\n",
            f = FIELD,
            r = RECORD
        );
        let commits = parse_log(&output);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].author, "Ana López");
        assert_eq!(commits[0].time, 1_700_000_000_000);
        assert_eq!(commits[0].summary, "Fix: login; add tests");
        assert_eq!(commits[1].summary, "");
        assert!(parse_log("").is_empty());
    }

    #[test]
    fn worktree_list() {
        let output = "worktree C:/work/app\nHEAD 111\nbranch refs/heads/main\n\n\
                      worktree C:/work/app-feature\nHEAD 222\nbranch refs/heads/feature/x\nlocked\n\n\
                      worktree C:/work/app-old\nHEAD 333\ndetached\nprunable gitdir file points to non-existent location\n";
        let list = parse_worktrees(output);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].branch.as_deref(), Some("main"));
        assert_eq!(list[1].path, "C:/work/app-feature");
        assert_eq!(list[1].branch.as_deref(), Some("feature/x"));
        assert!(list[1].locked);
        assert!(list[2].detached && list[2].prunable);
        assert_eq!(list[2].head.as_deref(), Some("333"));
    }
}
