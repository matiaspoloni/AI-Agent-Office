import { useEffect, useMemo, useState } from "react";
import type { AgentEvent } from "../bindings/AgentEvent";
import { errorMessage } from "../ipc/backend";
import { useBackend } from "../ipc/BackendContext";
import { actionAvailability, modeCapabilities } from "../state/capabilities";
import { describeEvent, eventTone } from "../state/describe";
import {
  ACTIVITY_COLOR,
  ACTIVITY_LABEL,
  formatCost,
  formatDuration,
  formatTime,
  formatUsage,
  isAvailable,
  shortPath,
} from "../state/format";
import { silentSince } from "../state/silence";
import { useOfficeStore } from "../state/store";
import { VirtualList } from "./VirtualList";

function useNow(intervalMs: number) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), intervalMs);
    return () => window.clearInterval(id);
  }, [intervalMs]);
  return now;
}

const EMPTY_EVENTS: AgentEvent[] = [];

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="kv">
      <span className="kv-label">{label}</span>
      <span className="kv-value">{children}</span>
    </div>
  );
}

export function AgentPanel() {
  const backend = useBackend();
  const key = useOfficeStore((s) => s.selectedAgentKey);
  const agent = useOfficeStore((s) => (key ? s.agents[key] : undefined));
  const session = useOfficeStore((s) => (agent ? s.sessions[agent.sessionKey] : undefined));
  const provider = useOfficeStore((s) => (agent ? s.providers[agent.provider] : undefined));
  const projects = useOfficeStore((s) => s.projects);
  const liveEvents = useOfficeStore((s) => (agent ? s.eventsBySession[agent.sessionKey] : undefined)) ?? EMPTY_EVENTS;
  const agents = useOfficeStore((s) => s.agents);
  const select = useOfficeStore((s) => s.select);
  const pushToast = useOfficeStore((s) => s.pushToast);
  const [history, setHistory] = useState<AgentEvent[]>([]);
  const [prompt, setPrompt] = useState("");
  const [onlyThisAgent, setOnlyThisAgent] = useState(false);
  const [busy, setBusy] = useState(false);
  // Restarting a running session stops it first: the button asks once more.
  const [confirmRestart, setConfirmRestart] = useState(false);
  const now = useNow(1000);

  useEffect(() => {
    if (!confirmRestart) return;
    const id = window.setTimeout(() => setConfirmRestart(false), 4000);
    return () => window.clearTimeout(id);
  }, [confirmRestart]);
  useEffect(() => setConfirmRestart(false), [agent?.sessionKey]);

  // Older events come from the database; newer ones stream in live.
  useEffect(() => {
    setHistory([]);
    if (!agent) return;
    let cancelled = false;
    backend
      .recentEvents(agent.sessionKey, 300)
      .then((events) => !cancelled && setHistory(events))
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [backend, agent?.sessionKey]);

  const events = useMemo(() => {
    const seen = new Set<string>();
    const merged: AgentEvent[] = [];
    for (const e of [...history, ...liveEvents]) {
      if (seen.has(e.eventId)) continue;
      seen.add(e.eventId);
      if (onlyThisAgent && agent && e.agentId !== agent.agentId) continue;
      merged.push(e);
    }
    merged.sort((a, b) => a.timestamp - b.timestamp);
    return merged;
  }, [history, liveEvents, onlyThisAgent, agent]);

  if (!agent || !session) return null;

  const descriptor = provider?.descriptor;
  const project = projects.find((p) => p.id === session.projectId);
  const children = Object.values(agents).filter((a) => a.parentKey === agent.key);
  const parent = agent.parentKey ? agents[agent.parentKey] : undefined;
  const { caps } = modeCapabilities(provider, session.mode);
  const preview = backend.kind === "preview";
  const previewOnly = { enabled: false, reason: "Actions need the desktop app (browser preview is a recording)" };
  const stop = preview ? previewOnly : actionAvailability(provider, session.mode, "stop");
  const send = preview ? previewOnly : actionAvailability(provider, session.mode, "sendPrompt");
  const perms = preview ? previewOnly : actionAvailability(provider, session.mode, "permissions");
  const resume = preview ? previewOnly : actionAvailability(provider, session.mode, "resume");
  const restart =
    resume.enabled && session.mode !== "managed"
      ? { enabled: false, reason: "Only sessions started from Agent Office can be restarted" }
      : resume.enabled
        ? {
            enabled: true,
            reason:
              session.status === "active"
                ? "Stops this agent, then continues the same conversation in a new process"
                : "Continues the same conversation in a new process",
          }
        : { enabled: false, reason: resume.reason };
  const terminal = preview
    ? previewOnly
    : session.cwd
      ? { enabled: true, reason: `Open your terminal in ${session.cwd}` }
      : { enabled: false, reason: "This session did not report its folder" };
  const quietSince = silentSince(agent, session);
  const pending = agent.pendingPermission;
  const canResolve = !!pending?.canResolve && perms.enabled;

  const run = async (label: string, action: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await action();
    } catch (error) {
      pushToast("error", `${label}: ${errorMessage(error)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <aside className="agent-panel" aria-label="Agent details">
      <header className="panel-header">
        <span className="badge" style={{ background: descriptor?.accentColor }}>
          {descriptor?.badge ?? "?"}
        </span>
        <div className="panel-title">
          <strong>{agent.name}</strong>
          <small>
            {descriptor?.displayName ?? agent.provider}
            {descriptor?.simulated ? " · simulated" : ""} · {session.mode}
          </small>
        </div>
        <button className="icon-button" onClick={() => select(null)} aria-label="Close panel">
          ✕
        </button>
      </header>

      <div className="status-pill" style={{ borderColor: ACTIVITY_COLOR[agent.activity] }}>
        <span className="dot" style={{ background: ACTIVITY_COLOR[agent.activity] }} />
        {ACTIVITY_LABEL[agent.activity]}
        <span className="muted"> · {formatDuration(now - agent.activitySince)}</span>
      </div>

      {quietSince !== null && (
        <div className="silence-box" role="status">
          <strong>No news for {formatDuration(now - quietSince)}</strong>
          <p className="small">
            This agent is busy but has not reported anything since {formatTime(quietSince)}. It may be waiting on a slow
            command, or it may be stuck. Agent Office will not stop it — use Stop if you want to.
          </p>
        </div>
      )}

      {pending && (
        <div className="permission-box">
          <strong>Permission requested</strong>
          <p>{pending.description}</p>
          {canResolve ? (
            <div className="button-row">
              <button
                className="btn good"
                disabled={busy}
                onClick={() =>
                  run("Approve", () =>
                    backend.resolvePermission(agent.provider, agent.sessionId, pending.requestId, {
                      decision: "approve",
                      forSession: false,
                    }),
                  )
                }
              >
                Approve
              </button>
              <button
                className="btn bad"
                disabled={busy}
                onClick={() =>
                  run("Reject", () =>
                    backend.resolvePermission(agent.provider, agent.sessionId, pending.requestId, {
                      decision: "reject",
                    }),
                  )
                }
              >
                Reject
              </button>
            </div>
          ) : (
            <p className="muted small">
              Answer this in the agent's own terminal.{" "}
              {!perms.enabled
                ? perms.reason
                : session.mode === "external"
                  ? "To answer from here, turn on “Answer permission requests of external sessions” in Diagnostics → Settings."
                  : "Agent Office cannot answer this request."}
            </p>
          )}
        </div>
      )}

      <section className="panel-section">
        <Row label="Agent">
          {agent.isMain ? "Main agent" : `Subagent${agent.agentType ? ` · ${agent.agentType}` : ""}`}
        </Row>
        {parent && (
          <Row label="Reports to">
            <button className="link" onClick={() => select(parent.key)}>
              {parent.name}
            </button>
          </Row>
        )}
        <Row label="Project">{project?.name ?? shortPath(session.cwd, 3)}</Row>
        <Row label="Model">{session.model ?? (caps && isAvailable(caps.model) ? "Not reported yet" : "Unavailable")}</Row>
        <Row label="Current action">{agent.currentAction ?? "—"}</Row>
        <Row label="Elapsed">{formatDuration((session.endedAt ?? now) - session.startedAt)}</Row>
        {session.status === "ended" && session.endReason && <Row label="Ended">{session.endReason}</Row>}
        {session.pid != null && agent.isMain && (
          <Row label="Process">
            PID {session.pid}
            {session.mode === "managed" ? " · started by Agent Office" : ""}
          </Row>
        )}
        {session.restarts > 0 && <Row label="Restarts">{session.restarts}</Row>}
        <Row label="Branch">{session.branch ?? "—"}</Row>
        <Row label="Files changed">{session.stats.filesChanged.length}</Row>
        <Row label="Tool calls">
          {agent.toolCalls} {agent.isMain ? `(session ${session.stats.toolCalls})` : ""}
        </Row>
        <Row label="Commands">{session.stats.commands}</Row>
        <Row label="Usage">{formatUsage(session.usage, !!caps && isAvailable(caps.usage))}</Row>
        <Row label="Cost">{formatCost(session.usage)}</Row>
        {agent.lastError && <Row label="Last error">{agent.lastError}</Row>}
      </section>

      {children.length > 0 && (
        <section className="panel-section">
          <h4>Subagents</h4>
          {children.map((c) => (
            <button key={c.key} className="subagent-row" onClick={() => select(c.key)}>
              <span className="dot" style={{ background: ACTIVITY_COLOR[c.activity] }} />
              {c.name} <span className="muted">· {ACTIVITY_LABEL[c.activity]}</span>
            </button>
          ))}
        </section>
      )}

      {session.stats.filesChanged.length > 0 && (
        <section className="panel-section">
          <h4>Changed files (tool evidence)</h4>
          <ul className="file-list">
            {session.stats.filesChanged.slice(0, 12).map((f) => (
              <li key={f} title={f}>
                {f}
              </li>
            ))}
          </ul>
        </section>
      )}

      <section className="panel-section">
        <h4>Actions</h4>
        <div className="button-row wrap">
          <button
            className="btn"
            disabled={!stop.enabled || busy || session.status === "ended"}
            title={stop.reason}
            onClick={() => run("Stop", () => backend.stopSession(agent.provider, agent.sessionId, false))}
          >
            Stop
          </button>
          <button
            className={`btn${confirmRestart ? " bad" : ""}`}
            disabled={!restart.enabled || busy}
            title={restart.reason}
            onClick={() => {
              if (session.status === "active" && !confirmRestart) {
                setConfirmRestart(true);
                return;
              }
              setConfirmRestart(false);
              void run("Restart", () => backend.restartSession(agent.provider, agent.sessionId));
            }}
          >
            {confirmRestart ? "Stop and restart?" : "Restart"}
          </button>
          <button
            className="btn"
            disabled={!terminal.enabled || busy}
            title={terminal.reason}
            onClick={() => run("Open terminal", () => backend.openTerminal(agent.provider, agent.sessionId))}
          >
            Open terminal
          </button>
          <button className="btn" disabled title="Opening folders/files arrives with the Git integration (Phase 8)">
            Open project
          </button>
        </div>
        <div className="prompt-box">
          <textarea
            placeholder={send.enabled ? "Send a prompt to this agent…" : send.reason}
            value={prompt}
            disabled={!send.enabled || session.status === "ended"}
            onChange={(e) => setPrompt(e.target.value)}
            rows={2}
          />
          <button
            className="btn primary"
            disabled={!send.enabled || !prompt.trim() || busy || session.status === "ended"}
            title={send.reason}
            onClick={() =>
              run("Send prompt", async () => {
                await backend.sendPrompt(agent.provider, agent.sessionId, prompt);
                setPrompt("");
              })
            }
          >
            Send
          </button>
        </div>
      </section>

      <section className="panel-section grow">
        <div className="log-header">
          <h4>Logs</h4>
          <label className="small">
            <input type="checkbox" checked={onlyThisAgent} onChange={(e) => setOnlyThisAgent(e.target.checked)} /> this
            agent only
          </label>
        </div>
        <VirtualList
          className="log"
          items={events}
          rowHeight={20}
          height={220}
          render={(e) => (
            <div className={`log-row ${eventTone(e)}`} title={describeEvent(e)}>
              <span className="log-time">{formatTime(e.timestamp)}</span>
              <span className="log-text">{describeEvent(e)}</span>
            </div>
          )}
        />
      </section>
    </aside>
  );
}
