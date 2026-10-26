import type { AgentEvent } from "../bindings/AgentEvent";
import type { WaitingReason } from "../bindings/WaitingReason";

/** One row in the conversation view. Built only from normalized events. */
export interface ChatItem {
  id: string;
  role: "user" | "agent" | "note";
  text: string;
  timestamp: number;
  tone?: "muted" | "good" | "bad" | "warn";
  /** Set when a subagent, not the agent the panel is showing, said it. */
  speaker?: string;
}

export interface PendingPrompt {
  id: string;
  text: string;
  at: number;
}

interface ToolBurst {
  id: string;
  timestamp: number;
  labels: string[];
  tone: "muted" | "bad";
}

type Piece =
  | { kind: "skip" }
  | { kind: "tool"; text: string; bad: boolean }
  | { kind: "user"; text: string }
  | { kind: "agent"; text: string }
  | { kind: "note"; text: string; tone: NonNullable<ChatItem["tone"]> };

const SKIP: Piece = { kind: "skip" };

function waitingText(reason: WaitingReason, message?: string): string {
  const custom = message?.trim();
  if (custom) return custom;
  switch (reason) {
    case "permission":
      return "Waiting for permission";
    case "input":
      return "Waiting for your answer";
    case "other":
      return "Waiting";
    default: {
      const unreachable: never = reason;
      return unreachable;
    }
  }
}

/** What a single event contributes to the conversation. Everything else stays in the log. */
function classify(e: AgentEvent): Piece {
  switch (e.type) {
    case "prompt.submitted":
      return { kind: "user", text: e.payload.text?.trim() || "Prompt sent (text not stored)" };
    case "agent.message": {
      const text = e.payload.text?.trim();
      return text ? { kind: "agent", text } : SKIP;
    }
    case "agent.error":
      return { kind: "note", text: e.payload.message, tone: "bad" };
    case "provider.error":
      return { kind: "note", text: e.payload.message, tone: "bad" };
    case "agent.waiting":
      return { kind: "note", text: waitingText(e.payload.reason, e.payload.message), tone: "warn" };
    case "permission.requested":
      return { kind: "note", text: `Permission: ${e.payload.description}`, tone: "warn" };
    case "permission.approved":
      return { kind: "note", text: "Permission approved", tone: "good" };
    case "permission.denied":
      return {
        kind: "note",
        text: e.payload.message ? `Permission denied: ${e.payload.message}` : "Permission denied",
        tone: "bad",
      };
    case "permission.expired":
      return {
        kind: "note",
        text: e.payload.message ?? "No answer in Agent Office; answer in the agent's own prompt",
        tone: "warn",
      };
    case "session.ended":
      return { kind: "note", text: e.payload.reason ? `Session ended (${e.payload.reason})` : "Session ended", tone: "muted" };
    case "subagent.started":
      return {
        kind: "note",
        text: `Subagent started: ${e.payload.agentType ?? "subagent"}${e.payload.description ? ` — ${e.payload.description}` : ""}`,
        tone: "muted",
      };
    case "context.compacted":
      return { kind: "note", text: "Context compacted", tone: "muted" };
    case "tool.started":
      return { kind: "tool", text: e.payload.title?.trim() || e.payload.toolName, bad: false };
    case "tool.failed":
      return {
        kind: "tool",
        text: e.payload.detail ? `Failed: ${e.payload.toolName} — ${e.payload.detail}` : `Failed: ${e.payload.toolName}`,
        bad: true,
      };
    case "command.failed":
      return {
        kind: "tool",
        text: e.payload.command ? `Command failed: ${e.payload.command}` : "Command failed",
        bad: true,
      };
    case "session.started":
    case "session.updated":
    case "agent.thinking":
    case "agent.idle":
    case "tool.completed":
    case "file.read":
    case "file.created":
    case "file.modified":
    case "file.deleted":
    case "command.started":
    case "command.output":
    case "command.completed":
    case "subagent.updated":
    case "subagent.ended":
    case "git.branch_changed":
    case "git.commit_created":
    case "git.status_changed":
    case "usage.updated":
      return SKIP;
    default: {
      const unreachable: never = e;
      return unreachable;
    }
  }
}

function flush(burst: ToolBurst | null, out: ChatItem[]): null {
  if (!burst) return null;
  const last = burst.labels[burst.labels.length - 1] ?? "";
  const text = burst.labels.length <= 3 ? burst.labels.join(" · ") : `${burst.labels.length} actions · last: ${last}`;
  out.push({ id: burst.id, role: "note", text, timestamp: burst.timestamp, tone: burst.tone });
  return null;
}

/**
 * Conversation for one session: the user's prompts, the agents' replies, and a
 * short note where a run of tool calls happened between them.
 * A reply that grows (the provider sends the same text again, longer) stays one bubble.
 */
export function conversationFromEvents(
  events: AgentEvent[],
  options: { focusAgentId?: string; names?: Readonly<Record<string, string>> } = {},
): ChatItem[] {
  const names = options.names ?? {};
  const focus = options.focusAgentId;
  const seen = new Set<string>();
  const ordered = [...events].sort((a, b) => a.timestamp - b.timestamp);
  const out: ChatItem[] = [];
  let burst: ToolBurst | null = null;

  for (const e of ordered) {
    if (seen.has(e.eventId)) continue;
    seen.add(e.eventId);
    const piece = classify(e);
    if (piece.kind === "skip") continue;
    if (piece.kind === "tool") {
      if (!burst) {
        burst = { id: e.eventId, timestamp: e.timestamp, labels: [piece.text], tone: piece.bad ? "bad" : "muted" };
      } else if (!burst.labels.includes(piece.text)) {
        burst.labels.push(piece.text);
        burst.timestamp = e.timestamp;
        if (piece.bad) burst.tone = "bad";
      }
      continue;
    }
    burst = flush(burst, out);
    if (piece.kind === "user") {
      out.push({ id: e.eventId, role: "user", text: piece.text, timestamp: e.timestamp });
      continue;
    }
    if (piece.kind === "note") {
      out.push({ id: e.eventId, role: "note", text: piece.text, timestamp: e.timestamp, tone: piece.tone });
      continue;
    }
    const speaker = focus && e.agentId !== focus ? (names[e.agentId] ?? e.agentId) : undefined;
    const item: ChatItem = { id: e.eventId, role: "agent", text: piece.text, timestamp: e.timestamp, speaker };
    const prev = out[out.length - 1];
    if (prev?.role === "agent" && prev.speaker === speaker) {
      if (item.text === prev.text || prev.text.startsWith(item.text)) continue;
      if (item.text.startsWith(prev.text)) {
        out[out.length - 1] = { ...item, id: prev.id };
        continue;
      }
    }
    out.push(item);
  }
  flush(burst, out);
  return out;
}

const PENDING_WINDOW_MS = 15_000;

/**
 * Keeps a prompt the user just typed visible until the matching
 * `prompt.submitted` event arrives. When that event has no text (prompt
 * storage is off), the local text replaces the placeholder.
 */
export function attachPending(items: ChatItem[], pending: readonly PendingPrompt[], events: readonly AgentEvent[]): ChatItem[] {
  if (!pending.length) return items;
  let next = items;
  const still: PendingPrompt[] = [];
  const consumed = new Set<string>();
  for (const p of pending) {
    const recent = events.filter(
      (e): e is AgentEvent & { type: "prompt.submitted" } =>
        e.type === "prompt.submitted" && e.timestamp >= p.at - PENDING_WINDOW_MS && !consumed.has(e.eventId),
    );
    const same = recent.find((e) => e.payload.text?.trim() === p.text);
    if (same) {
      consumed.add(same.eventId);
      continue;
    }
    const empty = recent.find((e) => !e.payload.text?.trim());
    if (empty) {
      consumed.add(empty.eventId);
      next = next.filter((item) => item.id !== empty.eventId);
    }
    still.push(p);
  }
  if (!still.length) return next;
  const extra: ChatItem[] = still.map((p) => ({ id: p.id, role: "user", text: p.text, timestamp: p.at }));
  return [...next, ...extra].sort((a, b) => a.timestamp - b.timestamp);
}
