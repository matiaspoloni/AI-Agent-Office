import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { Activity } from "../bindings/Activity";
import type { AgentEvent } from "../bindings/AgentEvent";
import { attachPending, conversationFromEvents, type PendingPrompt } from "../state/chat";
import { ACTIVITY_LABEL, formatTime } from "../state/format";

const WORKING: ReadonlySet<Activity> = new Set(["THINKING", "READING", "CODING", "RUNNING_COMMAND", "TESTING"]);

export function AgentChat({
  sessionKey,
  agentName,
  focusAgentId,
  names,
  events,
  activity,
  ended,
  send,
  busy,
  onSend,
}: {
  sessionKey: string;
  agentName: string;
  focusAgentId: string;
  names: Readonly<Record<string, string>>;
  events: AgentEvent[];
  activity: Activity;
  ended: boolean;
  send: { enabled: boolean; reason: string };
  busy: boolean;
  /** Rejects when the prompt was not delivered. The caller reports the error. */
  onSend: (text: string) => Promise<void>;
}) {
  const [text, setText] = useState("");
  const [pending, setPending] = useState<PendingPrompt[]>([]);
  const logRef = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    setText("");
    setPending([]);
  }, [sessionKey]);

  useEffect(() => {
    setPending((current) => {
      if (!current.length) return current;
      const next = current.filter(
        (p) => !events.some((e) => e.type === "prompt.submitted" && e.timestamp >= p.at - 15_000 && e.payload.text?.trim() === p.text),
      );
      return next.length === current.length ? current : next;
    });
  }, [events]);

  const items = useMemo(
    () => attachPending(conversationFromEvents(events, { focusAgentId, names }), pending, events),
    [events, focusAgentId, names, pending],
  );

  const typing = !ended && WORKING.has(activity);
  const lastAgent = [...items].reverse().find((item) => item.role === "agent");

  useLayoutEffect(() => {
    const el = logRef.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  }, [items, typing]);

  const submit = async () => {
    const prompt = text.trim();
    if (!prompt || !send.enabled || busy || ended) return;
    try {
      await onSend(prompt);
    } catch {
      return;
    }
    setPending((current) => [...current, { id: `local-${sessionKey}-${Date.now()}`, text: prompt, at: Date.now() }]);
    setText("");
    inputRef.current?.focus();
  };

  const canType = send.enabled && !ended;

  return (
    <section className="agent-chat" aria-label={`Conversation with ${agentName}`}>
      <h4>Conversation</h4>
      <div
        ref={logRef}
        className="agent-chat-log"
        role="log"
        onScroll={(e) => {
          const el = e.currentTarget;
          pinned.current = el.scrollTop + el.clientHeight >= el.scrollHeight - 24;
        }}
      >
        <div className="agent-chat-spacer" />
        {items.length === 0 && !typing && (
          <p className="agent-chat-empty muted small">
            No messages yet. Send one and {agentName}&apos;s reply shows up here.
          </p>
        )}
        {items.map((item) =>
          item.role === "note" ? (
            <p key={item.id} className={`agent-chat-note ${item.tone ?? "muted"}`}>
              {item.text}
            </p>
          ) : (
            <div key={item.id} className={`agent-chat-row ${item.role}`}>
              <span className="agent-chat-meta">
                {item.role === "user" ? "You" : (item.speaker ?? agentName)} · {formatTime(item.timestamp)}
              </span>
              <div className="agent-chat-bubble">{item.text}</div>
            </div>
          ),
        )}
        {typing && (
          <p className="agent-chat-typing" role="status">
            {agentName} · {ACTIVITY_LABEL[activity]}
          </p>
        )}
      </div>
      <div className="sr-only" aria-live="polite">
        {lastAgent ? `${lastAgent.speaker ?? agentName}: ${lastAgent.text}` : ""}
      </div>
      <div className="agent-chat-compose">
        <textarea
          ref={inputRef}
          rows={2}
          value={text}
          placeholder={canType ? `Message ${agentName}…` : send.reason}
          aria-label={`Message ${agentName}`}
          disabled={!canType || busy}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              void submit();
            }
          }}
        />
        <button className="btn primary" disabled={!canType || !text.trim() || busy} title={send.reason} onClick={() => void submit()}>
          Send
        </button>
      </div>
      {canType && <span className="muted small">Enter sends · Shift+Enter new line</span>}
    </section>
  );
}
