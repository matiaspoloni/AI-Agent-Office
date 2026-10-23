import { useEffect, useRef, useState } from "react";
import { errorMessage } from "../ipc/backend";
import { useBackend } from "../ipc/BackendContext";
import { continueAvailability, promptAvailability } from "../state/capabilities";
import { useOfficeStore } from "../state/store";

const PREVIEW_ONLY = {
  enabled: false,
  reason: "Instructions need the desktop app (the browser preview is a recording).",
};

/**
 * A speech bubble next to a character to give its session an instruction
 * without opening the panel. Prompts go to the session, so a subagent's
 * bubble talks to its lead. When the session cannot take prompts, the bubble
 * says why — and offers "Continue in Agent Office" for a session that ended
 * in the user's terminal.
 */
export function QuickPrompt({
  agentKey,
  onClose,
  ref,
}: {
  agentKey: string;
  onClose: () => void;
  ref?: React.Ref<HTMLDivElement>;
}) {
  const backend = useBackend();
  const agent = useOfficeStore((s) => s.agents[agentKey]);
  const session = useOfficeStore((s) => (agent ? s.sessions[agent.sessionKey] : undefined));
  const provider = useOfficeStore((s) => (agent ? s.providers[agent.provider] : undefined));
  const lead = useOfficeStore((s) => (session ? s.agents[session.mainAgentKey] : undefined));
  const pushToast = useOfficeStore((s) => s.pushToast);
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);

  const preview = backend.kind === "preview";
  const send = !session ? PREVIEW_ONLY : preview ? PREVIEW_ONLY : promptAvailability(provider, session);
  const continueHere = !session || preview ? PREVIEW_ONLY : continueAvailability(provider, session);

  // Focus the text box as soon as there is one (also right after a session
  // has been continued here), otherwise the close button.
  useEffect(() => {
    (send.enabled ? inputRef.current : closeRef.current)?.focus();
  }, [send.enabled]);

  if (!agent || !session) return null;
  const target = agent.isMain || !lead ? agent.name : lead.name;

  const submit = async () => {
    const prompt = text.trim();
    if (!prompt || !send.enabled || busy) return;
    setBusy(true);
    try {
      await backend.sendPrompt(agent.provider, agent.sessionId, prompt);
      pushToast("info", `Sent to ${target}.`);
      onClose();
    } catch (error) {
      pushToast("error", `Send prompt: ${errorMessage(error)}`);
    } finally {
      setBusy(false);
    }
  };

  const continueSession = async () => {
    setBusy(true);
    try {
      await backend.continueSession(agent.provider, agent.sessionId);
      pushToast("info", `${target} continues in Agent Office: you can send it prompts now.`);
    } catch (error) {
      pushToast("error", `Continue in Agent Office: ${errorMessage(error)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      className="office-quickprompt"
      ref={ref}
      role="dialog"
      aria-label={`Instruction for ${target}`}
      onKeyDown={(e) => {
        if (e.key === "Escape") {
          e.stopPropagation();
          onClose();
        }
      }}
    >
      <div className="office-quickprompt-title">
        <strong>Tell {target}</strong>
        {!agent.isMain && lead && <span className="muted small">(lead of {agent.name})</span>}
        <button ref={closeRef} className="btn small office-quickprompt-close" aria-label="Close" onClick={onClose}>
          ×
        </button>
      </div>
      {send.enabled ? (
        <>
          <textarea
            ref={inputRef}
            rows={2}
            value={text}
            placeholder="What should it do next?"
            aria-label={`Instruction for ${target}`}
            disabled={busy}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                void submit();
              }
            }}
          />
          <div className="office-quickprompt-row">
            <span className="muted small">Enter sends · Shift+Enter new line · Esc closes</span>
            <button className="btn primary small" disabled={!text.trim() || busy} onClick={() => void submit()}>
              Send
            </button>
          </div>
        </>
      ) : (
        <>
          <p className="small">{send.reason}</p>
          {session.mode === "external" && (
            <>
              <button
                className="btn small"
                disabled={!continueHere.enabled || busy}
                title={continueHere.reason}
                onClick={() => void continueSession()}
              >
                Continue in Agent Office
              </button>
              {!continueHere.enabled && <p className="muted small">{continueHere.reason}</p>}
            </>
          )}
        </>
      )}
    </div>
  );
}
