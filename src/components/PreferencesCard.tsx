import { useEffect, useState } from "react";
import type { Preferences } from "../bindings/Preferences";
import { errorMessage } from "../ipc/backend";
import { useBackend } from "../ipc/BackendContext";
import { useOfficeStore } from "../state/store";

/** Privacy and integration preferences (desktop only). */
export function PreferencesCard({ onSaved }: { onSaved?: () => void }) {
  const backend = useBackend();
  const pushToast = useOfficeStore((s) => s.pushToast);
  const [saved, setSaved] = useState<Preferences | null>(null);
  const [draft, setDraft] = useState<Preferences | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    backend
      .getPreferences()
      .then((p) => {
        setSaved(p);
        setDraft(p);
      })
      .catch((error) => pushToast("error", `Could not load settings: ${errorMessage(error)}`));
  }, [backend, pushToast]);

  if (!draft || !saved) return null;
  const set = <K extends keyof Preferences>(key: K, value: Preferences[K]) => setDraft({ ...draft, [key]: value });
  const dirty = JSON.stringify(draft) !== JSON.stringify(saved);

  const save = async () => {
    setBusy(true);
    try {
      const next = await backend.setPreferences(draft);
      setSaved(next);
      setDraft(next);
      pushToast("info", "Settings saved");
      onSaved?.();
    } catch (error) {
      pushToast("error", `Could not save settings: ${errorMessage(error)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="card">
      <h3>Settings</h3>
      <div className="grid-2">
        <div>
          <label className="check">
            <input
              type="checkbox"
              checked={draft.answerPermissionsFromApp}
              onChange={(e) => set("answerPermissionsFromApp", e.target.checked)}
            />
            <span>
              Answer permission requests of external sessions here
              <small className="muted">
                {" "}
                — the agent waits for Approve/Reject in Agent Office; its own prompt appears only if you don't answer in
                time. Off: Agent Office only shows the request. With Codex, changing this (or the wait below) updates one
                hook, which Codex asks you to trust again in /hooks.
              </small>
            </span>
          </label>
          <label>
            Wait for an answer (seconds)
            <input
              type="number"
              min={10}
              max={3600}
              value={draft.permissionTimeoutSecs}
              onChange={(e) => set("permissionTimeoutSecs", Number(e.target.value))}
            />
          </label>
          <label>
            Warn when a busy agent is silent for (minutes, 0 = never)
            <input
              type="number"
              min={0}
              max={1440}
              value={draft.silenceWarningMinutes}
              onChange={(e) => set("silenceWarningMinutes", Number(e.target.value))}
            />
          </label>
          <label className="check">
            <input
              type="checkbox"
              checked={draft.discoverExternalSessions}
              onChange={(e) => set("discoverExternalSessions", e.target.checked)}
            />
            <span>
              Discover running sessions
              <small className="muted"> — polls official listing commands such as `claude agents --json`.</small>
            </span>
          </label>
        </div>
        <div>
          <label className="check">
            <input type="checkbox" checked={draft.storePrompts} onChange={(e) => set("storePrompts", e.target.checked)} />
            <span>
              Store prompt text
              <small className="muted"> — off keeps only the fact that a prompt was sent.</small>
            </span>
          </label>
          <label>
            Keep event history (days)
            <input
              type="number"
              min={1}
              max={365}
              value={draft.retentionDays}
              onChange={(e) => set("retentionDays", Number(e.target.value))}
            />
          </label>
          <label>
            Maximum stored characters per text field
            <input
              type="number"
              min={256}
              max={262144}
              value={draft.maxOutputChars}
              onChange={(e) => set("maxOutputChars", Number(e.target.value))}
            />
          </label>
        </div>
      </div>
      <div className="button-row">
        <button className="btn primary" disabled={!dirty || busy} onClick={save}>
          Save settings
        </button>
        {dirty && (
          <button className="btn" disabled={busy} onClick={() => setDraft(saved)}>
            Discard
          </button>
        )}
        <span className="muted small">Everything stays on this computer.</span>
      </div>
    </section>
  );
}
