import { describe, expect, it } from "vitest";
import type { AgentState } from "../bindings/AgentState";
import type { Project } from "../bindings/Project";
import type { SessionState } from "../bindings/SessionState";
import { folderName, projectForPath, teamResolver } from "./teams";

const project = (id: string, name: string, path: string): Project => ({ id, name, path, settings: {}, createdAt: 0, updatedAt: 0 });
const session = (key: string, partial: Partial<SessionState>): SessionState => ({
  key,
  provider: "claude",
  sessionId: key,
  mode: "external",
  status: "active",
  mainAgentKey: `${key}:main`,
  agentKeys: [],
  startedAt: 0,
  lastEventAt: 0,
  stats: { prompts: 0, toolCalls: 0, failedTools: 0, commands: 0, errors: 0, subagents: 0, commits: 0, filesChanged: [] },
  restarts: 0,
  ...partial,
});
const agent = (sessionKey: string): AgentState => ({
  key: `${sessionKey}:main`,
  sessionKey,
  provider: "claude",
  sessionId: sessionKey,
  agentId: "main",
  isMain: true,
  name: "Agent",
  activity: "CODING",
  activitySince: 0,
  runningTools: [],
  toolCalls: 0,
  ended: false,
});

describe("teams", () => {
  const projects = [
    project("p1", "Munder Difflin", "C:\\Projects\\Munder-Difflin"),
    project("p2", "Nalu", "C:\\Users\\matia\\OneDrive\\Desktop\\Nalu Claude"),
    project("p3", "Munder web", "C:\\Projects\\Munder-Difflin\\web"),
  ];

  it("finds the project containing a folder (longest match, any slash, any case)", () => {
    expect(projectForPath("c:/projects/munder-difflin/src", projects)?.id).toBe("p1");
    expect(projectForPath("C:\\Projects\\Munder-Difflin\\web\\app", projects)?.id).toBe("p3");
    expect(projectForPath("C:\\Projects\\Munder-Difflin-2", projects)).toBeUndefined();
    expect(projectForPath(undefined, projects)).toBeUndefined();
    expect(folderName("C:\\Projects\\Atlas\\")).toBe("Atlas");
  });

  it("groups agents by project, then by folder", () => {
    const sessions = {
      a: session("a", { projectId: "p2", cwd: "D:\\elsewhere" }),
      b: session("b", { cwd: "C:\\Projects\\Munder-Difflin\\tests" }),
      c: session("c", { cwd: "/home/u/Atlas" }),
      d: session("d", { title: "Loose session" }),
    };
    const resolve = teamResolver(sessions, projects);
    expect(resolve(agent("a"))).toEqual({ key: "project:p2", name: "Nalu" });
    expect(resolve(agent("b"))).toEqual({ key: "project:p1", name: "Munder Difflin" });
    expect(resolve(agent("c"))).toEqual({ key: "folder:atlas", name: "Atlas" });
    expect(resolve(agent("d"))).toEqual({ key: "session:d", name: "Loose session" });
  });
});
