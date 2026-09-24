import { useMemo, useState } from "react";
import { errorMessage } from "../ipc/backend";
import { useBackend } from "../ipc/BackendContext";
import { actionAvailability } from "../state/capabilities";
import { LAUNCH_HINTS } from "../state/launchHints";
import { useOfficeStore } from "../state/store";

export function NewAgentDialog({ onClose }: { onClose: () => void }) {
  const backend = useBackend();
  const providers = useOfficeStore((s) => s.providers);
  const order = useOfficeStore((s) => s.providerOrder);
  const projects = useOfficeStore((s) => s.projects);
  const pushToast = useOfficeStore((s) => s.pushToast);

  const options = useMemo(
    () =>
      order.map((id) => {
        const info = providers[id];
        return { id, info, launch: actionAvailability(info, "managed", "launch") };
      }),
    [order, providers],
  );
  const firstEnabled = options.find((o) => o.launch.enabled)?.id ?? order[0] ?? "";
  const [provider, setProvider] = useState(firstEnabled);
  const [projectId, setProjectId] = useState(projects[0]?.id ?? "");
  const [cwd, setCwd] = useState("");
  const [model, setModel] = useState("");
  const [name, setName] = useState("");
  const [prompt, setPrompt] = useState("");
  const [permissionMode, setPermissionMode] = useState("");
  const [busy, setBusy] = useState(false);

  const selected = options.find((o) => o.id === provider);
  const project = projects.find((p) => p.id === projectId);
  const folder = project?.path ?? cwd;
  const hints = LAUNCH_HINTS[provider];

  const launch = async () => {
    setBusy(true);
    try {
      await backend.launchSession(provider, {
        projectId: project?.id,
        cwd: folder,
        model: model.trim() || project?.defaultModel || undefined,
        name: name.trim() || undefined,
        prompt: prompt.trim() || undefined,
        permissionMode: (hints?.permissionModes.length && permissionMode) || undefined,
      });
      pushToast("info", `Launched ${selected?.info?.descriptor.displayName ?? provider}`);
      onClose();
    } catch (error) {
      pushToast("error", `Launch failed: ${errorMessage(error)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="modal-backdrop" role="presentation" onClick={onClose}>
      <div className="modal" role="dialog" aria-label="New agent" onClick={(e) => e.stopPropagation()}>
        <h3>New agent</h3>
        <label>
          Provider
          <select
            value={provider}
            onChange={(e) => {
              setProvider(e.target.value);
              setPermissionMode("");
            }}
          >
            {options.map((o) => (
              <option key={o.id} value={o.id}>
                {o.info?.descriptor.displayName ?? o.id}
                {o.launch.enabled ? "" : " — not available yet"}
              </option>
            ))}
          </select>
        </label>
        {selected && !selected.launch.enabled && <p className="notice">{selected.launch.reason}</p>}
        {selected?.info?.descriptor.simulated && (
          <p className="notice subtle">Demo agents are simulated: no provider is contacted and no files change.</p>
        )}

        <label>
          Project
          <select value={projectId} onChange={(e) => setProjectId(e.target.value)}>
            <option value="">Custom folder…</option>
            {projects.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
        </label>
        {!project && (
          <label>
            Folder
            <div className="input-row">
              <input value={cwd} onChange={(e) => setCwd(e.target.value)} placeholder="C:\\Projects\\MyApp" />
              {backend.kind === "desktop" && (
                <button className="btn" onClick={async () => setCwd((await backend.pickFolder()) ?? cwd)}>
                  Browse
                </button>
              )}
            </div>
          </label>
        )}
        <div className="grid-2">
          <label>
            Model <small className="muted">(optional)</small>
            <input
              value={model}
              onChange={(e) => setModel(e.target.value)}
              placeholder={project?.defaultModel ?? "provider default"}
              list={hints?.models.length ? `models-${provider}` : undefined}
            />
            {hints && hints.models.length > 0 && (
              <datalist id={`models-${provider}`}>
                {hints.models.map((m) => (
                  <option key={m} value={m} />
                ))}
              </datalist>
            )}
          </label>
          <label>
            Name <small className="muted">(optional)</small>
            <input value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. Backend refactor" />
          </label>
        </div>
        {hints && hints.permissionModes.length > 0 && (
          <label>
            Permission mode
            <select value={permissionMode} onChange={(e) => setPermissionMode(e.target.value)}>
              <option value="">Use my {selected?.info?.descriptor.displayName ?? provider} settings</option>
              {hints.permissionModes.map((m) => (
                <option key={m.value} value={m.value}>
                  {m.label}
                </option>
              ))}
            </select>
            <small className="muted">
              Permission requests appear on the agent in the office; answer them with Approve or Reject.
              {hints.note && ` ${hints.note}`}
            </small>
          </label>
        )}
        {permissionMode === "bypassPermissions" && (
          <p className="notice">The agent will run every tool without asking, including shell commands.</p>
        )}
        <label>
          First prompt <small className="muted">(optional)</small>
          <textarea rows={3} value={prompt} onChange={(e) => setPrompt(e.target.value)} />
        </label>
        <div className="button-row end">
          <button className="btn" onClick={onClose}>
            Cancel
          </button>
          <button className="btn primary" disabled={!selected?.launch.enabled || busy} onClick={launch}>
            Launch
          </button>
        </div>
      </div>
    </div>
  );
}
