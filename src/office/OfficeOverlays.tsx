// HTML overlays over the office canvas that need real controls: approve /
// reject chips next to agents waiting for permission, the "who to show"
// filter, and the strip of recent attention events.

import { useEffect, useMemo, useState } from "react";
import type { AgentEvent } from "../bindings/AgentEvent";
import { errorMessage } from "../ipc/backend";
import { useBackend } from "../ipc/BackendContext";
import { actionAvailability } from "../state/capabilities";
import { formatTime } from "../state/format";
import { useOfficeStore } from "../state/store";
import type { OfficeFilter } from "./renderer";
import { ICON_PALETTE, ICONS, scale2x, spriteCanvas } from "./sprites";
import { teamResolver } from "./teams";
import { markLeftPercent, timelineLabel, timelineWindow } from "./timeline";

/** A pixel icon from the office set, as an image URL. */
function iconUrl(name: string): string {
  return spriteCanvas(scale2x(ICONS[name]), ICON_PALETTE, `icon2x:${name}`)?.toDataURL() ?? "";
}

/**
 * Approve / Reject beside every agent with a pending permission request. The
 * office loop positions each chip next to its character via `refs`.
 */
export function PermissionChips({ refs }: { refs: React.RefObject<Map<string, HTMLDivElement>> }) {
  const backend = useBackend();
  const agents = useOfficeStore((s) => s.agents);
  const sessions = useOfficeStore((s) => s.sessions);
  const providers = useOfficeStore((s) => s.providers);
  const pushToast = useOfficeStore((s) => s.pushToast);
  const [busy, setBusy] = useState<string | null>(null);
  const waiting = Object.values(agents).filter((a) => a.pendingPermission && !a.ended);

  const answer = async (key: string, decision: "approve" | "reject") => {
    const agent = agents[key];
    const pending = agent?.pendingPermission;
    if (!agent || !pending) return;
    setBusy(key);
    try {
      await backend.resolvePermission(
        agent.provider,
        agent.sessionId,
        pending.requestId,
        decision === "approve" ? { decision, forSession: false } : { decision },
      );
    } catch (error) {
      pushToast("error", `${decision === "approve" ? "Approve" : "Reject"} failed: ${errorMessage(error)}`);
    } finally {
      setBusy(null);
    }
  };

  return (
    <>
      {waiting.map((agent) => {
        const session = sessions[agent.sessionKey];
        const perms =
          backend.kind === "preview"
            ? { enabled: false, reason: "Actions need the desktop app (browser preview is a recording)" }
            : session
              ? actionAvailability(providers[agent.provider], session.mode, "permissions")
              : { enabled: false, reason: "Unknown session" };
        const can = !!agent.pendingPermission?.canResolve && perms.enabled;
        const reason = can ? agent.pendingPermission?.description ?? "" : perms.enabled ? "Answer this in the agent's own terminal" : perms.reason;
        return (
          <div
            key={agent.key}
            className="office-chip"
            role="group"
            aria-label={`Permission requested by ${agent.name}`}
            ref={(el) => {
              if (el) refs.current?.set(agent.key, el);
              else refs.current?.delete(agent.key);
            }}
          >
            <button className="btn small good" disabled={!can || busy === agent.key} title={reason} aria-label={`Approve ${agent.name}`} onClick={() => answer(agent.key, "approve")}>
              <img src={iconUrl("check")} alt="" width={14} height={14} className="pixel-icon" />
            </button>
            <button className="btn small bad" disabled={!can || busy === agent.key} title={reason} aria-label={`Reject ${agent.name}`} onClick={() => answer(agent.key, "reject")}>
              <img src={iconUrl("error")} alt="" width={14} height={14} className="pixel-icon" />
            </button>
          </div>
        );
      })}
    </>
  );
}

/** Everyone, only those who need you, or one project. */
export function FilterControl({ value, onChange }: { value: OfficeFilter | null; onChange: (f: OfficeFilter | null) => void }) {
  const agents = useOfficeStore((s) => s.agents);
  const sessions = useOfficeStore((s) => s.sessions);
  const projects = useOfficeStore((s) => s.projects);
  const teams = useMemo(() => {
    const resolve = teamResolver(sessions, projects);
    const byKey = new Map<string, string>();
    for (const a of Object.values(agents)) if (!a.ended) byKey.set(resolve(a).key, resolve(a).name);
    return [...byKey].map(([key, name]) => ({ key, name })).sort((a, b) => a.name.localeCompare(b.name));
  }, [agents, sessions, projects]);
  const current = !value ? "all" : value.kind === "attention" ? "attention" : `team:${value.team}`;
  return (
    <label className="office-filter">
      <span className="sr-only">Show</span>
      <select
        value={current}
        onChange={(e) => {
          const v = e.target.value;
          onChange(v === "all" ? null : v === "attention" ? { kind: "attention" } : { kind: "team", team: v.slice(5) });
        }}
      >
        <option value="all">Everyone</option>
        <option value="attention">Needs me</option>
        {teams.map((t) => (
          <option key={t.key} value={`team:${t.key}`}>
            {t.name}
          </option>
        ))}
      </select>
    </label>
  );
}

type Mark = { id: string; agentKey: string; at: number; kind: "permission" | "error" | "waiting" | "finished"; label: string };

/** When this module loaded: the office's open clock for the attention strip. */
const officeOpenedAt = Date.now();

const MARK_LABEL: Record<Mark["kind"], string> = {
  permission: "asked for permission",
  error: "hit an error",
  waiting: "waited for an answer",
  finished: "finished",
};

function markOf(e: AgentEvent): Mark["kind"] | null {
  switch (e.type) {
    case "permission.requested":
      return "permission";
    case "agent.error":
    case "tool.failed":
    case "command.failed":
      return "error";
    case "agent.waiting":
      return "waiting";
    case "session.ended":
      return "finished";
    default:
      return null;
  }
}

/**
 * Moments that needed you since the office opened, as marks on a strip; click
 * one to select that agent. The strip grows with that open time and never
 * spans a full two hours. Only events received since Agent Office opened.
 */
export function OfficeTimeline() {
  const eventsBySession = useOfficeStore((s) => s.eventsBySession);
  const agents = useOfficeStore((s) => s.agents);
  const select = useOfficeStore((s) => s.select);
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, []);
  const marks = useMemo(() => {
    const byAgent = new Map<string, string>();
    for (const a of Object.values(agents)) byAgent.set(`${a.sessionKey}|${a.agentId}`, a.key);
    const out: Mark[] = [];
    for (const [sessionKey, events] of Object.entries(eventsBySession)) {
      for (const e of events) {
        const kind = markOf(e);
        const agentKey = byAgent.get(`${sessionKey}|${e.agentId}`);
        if (!kind || !agentKey) continue;
        out.push({ id: e.eventId, agentKey, at: e.timestamp, kind, label: `${agents[agentKey]?.name ?? "Agent"} ${MARK_LABEL[kind]} at ${formatTime(e.timestamp)}` });
      }
    }
    return out.sort((a, b) => a.at - b.at).slice(-120);
  }, [eventsBySession, agents]);
  const latest = marks.at(-1)?.at ?? now;
  const { start, end } = timelineWindow(officeOpenedAt, now, latest);
  const visible = marks.filter((m) => m.at >= start && m.at <= end);
  const label = timelineLabel(officeOpenedAt, now, latest);
  return (
    <div
      className="office-timeline"
      role="group"
      aria-label={`Moments that needed you since the office opened, ${label}`}
      title="Time since the office opened. The strip stays under two hours."
    >
      <span className="office-timeline-label small muted">{label}</span>
      <div className="office-timeline-track">
        {visible.map((m) => (
          <button
            key={m.id}
            className={`office-mark ${m.kind}`}
            style={{ left: `${markLeftPercent(m.at, start, end)}%` }}
            title={m.label}
            aria-label={m.label}
            onClick={() => select(m.agentKey)}
          />
        ))}
      </div>
      {visible.length === 0 && <span className="small muted">Nothing needed you yet</span>}
    </div>
  );
}
