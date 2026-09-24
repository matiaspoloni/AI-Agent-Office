// Which project team an agent belongs to, for the desk pods in the office.
// A session's project is the one the core resolved (projectId); otherwise
// the registered project whose folder contains the session's folder; the
// last resort is the folder name itself.

import type { AgentState } from "../bindings/AgentState";
import type { Project } from "../bindings/Project";
import type { SessionState } from "../bindings/SessionState";
import type { Team, TeamResolver } from "./scene";

function normalize(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
}

export function folderName(path: string | undefined): string | undefined {
  if (!path) return undefined;
  const parts = path.replace(/[\\/]+$/, "").split(/[\\/]/);
  return parts[parts.length - 1] || undefined;
}

/** The registered project containing `cwd` (longest matching folder wins). */
export function projectForPath(cwd: string | undefined, projects: Project[]): Project | undefined {
  if (!cwd) return undefined;
  const target = normalize(cwd);
  let best: Project | undefined;
  for (const p of projects) {
    const root = normalize(p.path);
    if (!root) continue;
    if ((target === root || target.startsWith(`${root}/`)) && (!best || root.length > normalize(best.path).length)) best = p;
  }
  return best;
}

export function teamResolver(sessions: Record<string, SessionState>, projects: Project[]): TeamResolver {
  const byId = new Map(projects.map((p) => [p.id, p]));
  const cache = new Map<string, Team>();
  return (agent: AgentState): Team => {
    const hit = cache.get(agent.sessionKey);
    if (hit) return hit;
    const session = sessions[agent.sessionKey];
    const project = (session?.projectId && byId.get(session.projectId)) || projectForPath(session?.cwd, projects);
    let team: Team;
    if (project) team = { key: `project:${project.id}`, name: project.name };
    else {
      const folder = folderName(session?.cwd);
      team = folder
        ? { key: `folder:${folder.toLowerCase()}`, name: folder }
        : { key: `session:${agent.sessionKey}`, name: session?.title ?? agent.name };
    }
    cache.set(agent.sessionKey, team);
    return team;
  };
}
