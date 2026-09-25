import { useMemo, useState } from "react";
import { errorMessage } from "../ipc/backend";
import { useBackend } from "../ipc/BackendContext";
import { actionAvailability } from "../state/capabilities";
import { ACTIVITY_COLOR, ACTIVITY_LABEL, formatTokens, shortPath } from "../state/format";
import { useOfficeStore } from "../state/store";
import { formatGitStatus } from "../state/git";

function Kpi({ label, value, tone }: { label: string; value: string | number; tone?: "warn" | "bad" | "good" }) {
  return (
    <div className={`kpi ${tone ?? ""}`}>
      <span className="kpi-value">{value}</span>
      <span className="kpi-label">{label}</span>
    </div>
  );
}

export function CommandCenter() {
  const backend = useBackend();
  const sessions = useOfficeStore((s) => s.sessions);
  const agents = useOfficeStore((s) => s.agents);
  const providers = useOfficeStore((s) => s.providers);
  const projects = useOfficeStore((s) => s.projects);
  const select = useOfficeStore((s) => s.select);
  const setView = useOfficeStore((s) => s.setView);
  const pushToast = useOfficeStore((s) => s.pushToast);
  const [busyKey, setBusyKey] = useState<string | null>(null);

  const data = useMemo(() => {
    const all = Object.values(agents);
    const active = all.filter((a) => !a.ended);
    const waiting = active.filter((a) => a.pendingPermission || a.activity === "WAITING_INPUT");
    const errors = active.filter((a) => a.activity === "ERROR");
    const sessionList = Object.values(sessions);
    const completed = sessionList.filter((s) => s.status === "ended");
    const branches = new Set(sessionList.map((s) => s.branch).filter(Boolean));
    // Registered projects, or the working folder for sessions outside any registered project.
    const projectIds = new Set(sessionList.map((s) => s.projectId ?? s.cwd).filter(Boolean));
    const toolCalls = sessionList.reduce((n, s) => n + s.stats.toolCalls, 0);
    let tokens = 0;
    let reporting = 0;
    for (const s of sessionList) {
      const t = s.usage?.totalTokens ?? (s.usage?.inputTokens ?? 0) + (s.usage?.outputTokens ?? 0);
      if (s.usage && t > 0) {
        tokens += t;
        reporting++;
      }
    }
    return { active, waiting, errors, completed, branches, projectIds, toolCalls, tokens, reporting, sessionList };
  }, [agents, sessions]);

  const open = (key: string) => {
    select(key);
    setView("office");
  };

  const resolve = async (agentKey: string, approve: boolean) => {
    const agent = agents[agentKey];
    const request = agent?.pendingPermission;
    if (!agent || !request) return;
    setBusyKey(agentKey);
    try {
      await backend.resolvePermission(
        agent.provider,
        agent.sessionId,
        request.requestId,
        approve ? { decision: "approve", forSession: false } : { decision: "reject" },
      );
    } catch (error) {
      pushToast("error", errorMessage(error));
    } finally {
      setBusyKey(null);
    }
  };

  return (
    <div className="command-center">
      <div className="kpis">
        <Kpi label="Active agents" value={data.active.length} />
        <Kpi label="Waiting on you" value={data.waiting.length} tone={data.waiting.length ? "warn" : undefined} />
        <Kpi label="Errors" value={data.errors.length} tone={data.errors.length ? "bad" : undefined} />
        <Kpi label="Completed sessions" value={data.completed.length} tone="good" />
        <Kpi label="Projects" value={data.projectIds.size} />
        <Kpi label="Branches" value={data.branches.size} />
        <Kpi label="Tool calls" value={data.toolCalls} />
        <Kpi
          label={data.reporting ? `Tokens (${data.reporting} session${data.reporting > 1 ? "s" : ""} reporting)` : "Tokens"}
          value={data.reporting ? formatTokens(data.tokens) : "Unavailable"}
        />
      </div>

      <section className="card">
        <h3>Waiting on you (approvals and questions)</h3>
        {data.waiting.length === 0 && <p className="muted">Nobody is waiting for you.</p>}
        {data.waiting.map((a) => {
          const session = sessions[a.sessionKey];
          const perms = actionAvailability(providers[a.provider], session?.mode ?? "external", "permissions");
          const canResolve = a.pendingPermission?.canResolve && perms.enabled;
          return (
            <div key={a.key} className="row">
              <span className="badge" style={{ background: providers[a.provider]?.descriptor.accentColor }}>
                {providers[a.provider]?.descriptor.badge}
              </span>
              <button className="link" onClick={() => open(a.key)}>
                {a.name}
              </button>
              <span className="grow">{a.pendingPermission?.description ?? a.currentAction}</span>
              {canResolve ? (
                <>
                  <button className="btn good" disabled={busyKey === a.key} onClick={() => resolve(a.key, true)}>
                    Approve
                  </button>
                  <button className="btn bad" disabled={busyKey === a.key} onClick={() => resolve(a.key, false)}>
                    Reject
                  </button>
                </>
              ) : (
                <span className="muted small" title={perms.reason}>
                  answer in the agent's terminal
                </span>
              )}
            </div>
          );
        })}
      </section>

      <section className="card">
        <h3>Active agents</h3>
        {data.active.length === 0 && <p className="muted">No active agents. Launch one or start the demo office.</p>}
        <table className="table">
          <thead>
            <tr>
              <th>Agent</th>
              <th>Provider</th>
              <th>Project</th>
              <th>Status</th>
              <th>Current action</th>
              <th>Branch</th>
              <th>Git changes</th>
              <th>Tools</th>
            </tr>
          </thead>
          <tbody>
            {data.active.map((a) => {
              const s = sessions[a.sessionKey];
              const project = projects.find((p) => p.id === s?.projectId);
              return (
                <tr key={a.key} onClick={() => open(a.key)}>
                  <td>
                    {a.isMain ? "" : "↳ "}
                    {a.name}
                  </td>
                  <td>{providers[a.provider]?.descriptor.displayName ?? a.provider}</td>
                  <td>{project?.name ?? shortPath(s?.cwd)}</td>
                  <td>
                    <span className="dot" style={{ background: ACTIVITY_COLOR[a.activity] }} /> {ACTIVITY_LABEL[a.activity]}
                  </td>
                  <td className="ellipsis">{a.currentAction ?? "—"}</td>
                  <td>{s?.branch ?? "—"}</td>
                  <td className="small">{a.isMain && s?.gitStatus ? formatGitStatus(s.gitStatus) : "—"}</td>
                  <td>{a.toolCalls}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </section>

      <div className="grid-2">
        <section className="card">
          <h3>Errors</h3>
          {data.errors.length === 0 && <p className="muted">No errors.</p>}
          {data.errors.map((a) => (
            <div key={a.key} className="row">
              <button className="link" onClick={() => open(a.key)}>
                {a.name}
              </button>
              <span className="grow bad-text">{a.lastError}</span>
            </div>
          ))}
        </section>
        <section className="card">
          <h3>Completed sessions</h3>
          {data.completed.length === 0 && <p className="muted">Nothing completed yet.</p>}
          {data.completed.slice(-8).map((s) => (
            <div key={s.key} className="row">
              <span className="grow">{s.title ?? s.sessionId}</span>
              <span className="muted small">{s.endReason ?? "ended"}</span>
            </div>
          ))}
        </section>
      </div>
    </div>
  );
}
