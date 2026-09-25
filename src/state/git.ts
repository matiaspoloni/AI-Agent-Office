import type { ChangeKind } from "../bindings/ChangeKind";
import type { FileChange } from "../bindings/FileChange";
import type { GitStatusChanged } from "../bindings/GitStatusChanged";
import type { WorktreeStatus } from "../bindings/WorktreeStatus";

/** Last folder name of a path ("C:\\Projects\\Nalu" → "Nalu"). */
export function folderName(path: string | undefined): string {
  if (!path) return "—";
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

const LETTER: Record<ChangeKind, string> = {
  modified: "M",
  typeChanged: "T",
  added: "A",
  deleted: "D",
  renamed: "R",
  copied: "C",
  untracked: "?",
  conflicted: "U",
};

const WORD: Record<ChangeKind, string> = {
  modified: "modified",
  typeChanged: "type changed",
  added: "added",
  deleted: "deleted",
  renamed: "renamed",
  copied: "copied",
  untracked: "new",
  conflicted: "conflict",
};

/** Git-style two-letter code (staged, then not staged) and a plain description. */
export function describeChange(file: FileChange): { code: string; text: string } {
  const code = `${file.staged ? LETTER[file.staged] : " "}${file.unstaged ? LETTER[file.unstaged] : " "}`;
  const parts: string[] = [];
  if (file.unstaged === "untracked") parts.push("new, not tracked yet");
  else if (file.unstaged === "conflicted") parts.push("merge conflict");
  else {
    if (file.staged) parts.push(`${WORD[file.staged]} (staged)`);
    if (file.unstaged) parts.push(`${WORD[file.unstaged]} (not staged)`);
  }
  return { code, text: parts.join(", ") };
}

/** "↑2 ↓1" when the upstream is known, else null. */
export function aheadBehind(s: { ahead?: number; behind?: number }): string | null {
  if (s.ahead === undefined && s.behind === undefined) return null;
  return `↑${s.ahead ?? 0} ↓${s.behind ?? 0}`;
}

/** One line for a session's Git status event. */
export function formatGitStatus(s: GitStatusChanged): string {
  const parts: string[] = [];
  const changed = s.dirty - (s.untracked ?? 0) - (s.conflicted ?? 0);
  if (changed > 0) parts.push(`${changed} changed`);
  if (s.untracked) parts.push(`${s.untracked} new`);
  if (s.conflicted) parts.push(`${s.conflicted} in conflict`);
  if (s.staged) parts.push(`${s.staged} staged`);
  if (parts.length === 0) parts.push("clean");
  const ab = aheadBehind(s);
  return ab ? `${parts.join(" · ")} · ${ab}` : parts.join(" · ");
}

/** Branch text for a working tree. */
export function branchLabel(status: WorktreeStatus | undefined, fallback?: string): string {
  if (status?.branch) return status.branch;
  if (status?.detached) return status.head ? `detached at ${status.head.slice(0, 7)}` : "detached";
  return fallback ?? "—";
}

/** Oldest Git Agent Office can use (same as `ao_git::MIN_VERSION`). */
export const MIN_GIT_VERSION: [number, number] = [2, 15];

/** Whether a version like "2.45.1.windows.1" is new enough. */
export function gitVersionSupported(version: string | undefined): boolean {
  if (!version) return false;
  const [major = 0, minor = 0] = version.split(".").map((p) => Number.parseInt(p, 10) || 0);
  return major > MIN_GIT_VERSION[0] || (major === MIN_GIT_VERSION[0] && minor >= MIN_GIT_VERSION[1]);
}
