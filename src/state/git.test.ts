import { describe, expect, it } from "vitest";
import { aheadBehind, branchLabel, describeChange, folderName, formatGitStatus, gitVersionSupported } from "./git";

describe("git formatting", () => {
  it("describes changed files like git status, in words", () => {
    expect(describeChange({ path: "a", unstaged: "modified" })).toEqual({ code: " M", text: "modified (not staged)" });
    expect(describeChange({ path: "a", staged: "added", unstaged: "modified" }).code).toBe("AM");
    expect(describeChange({ path: "a", unstaged: "untracked" }).text).toBe("new, not tracked yet");
    expect(describeChange({ path: "a", unstaged: "conflicted" }).code).toBe(" U");
    expect(describeChange({ path: "b", origPath: "a", staged: "renamed" }).text).toBe("renamed (staged)");
  });

  it("summarises a status without inventing numbers", () => {
    expect(formatGitStatus({ dirty: 0, staged: 0 })).toBe("clean");
    expect(formatGitStatus({ dirty: 4, staged: 1, untracked: 1, conflicted: 1, ahead: 2, behind: 0 })).toBe(
      "2 changed · 1 new · 1 in conflict · 1 staged · ↑2 ↓0",
    );
    // No upstream: no arrows at all.
    expect(aheadBehind({})).toBeNull();
  });

  it("names folders and branches", () => {
    expect(folderName("C:\\Projects\\Nalu\\")).toBe("Nalu");
    expect(folderName("/work/app")).toBe("app");
    const base = { detached: false, counts: { staged: 0, unstaged: 0, untracked: 0, conflicted: 0 }, files: [], filesTruncated: false };
    expect(branchLabel({ ...base, branch: "main" })).toBe("main");
    expect(branchLabel({ ...base, detached: true, head: "abcdef123456" })).toBe("detached at abcdef1");
    expect(branchLabel(undefined, "feature")).toBe("feature");
  });

  it("checks the Git version", () => {
    expect(gitVersionSupported("2.45.1.windows.1")).toBe(true);
    expect(gitVersionSupported("2.15.0")).toBe(true);
    expect(gitVersionSupported("2.14.9")).toBe(false);
    expect(gitVersionSupported("3.0")).toBe(true);
    expect(gitVersionSupported(undefined)).toBe(false);
  });
});
