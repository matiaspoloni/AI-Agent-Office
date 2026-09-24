import { useEffect, useState } from "react";
import type { IntegrationAction } from "../bindings/IntegrationAction";
import type { IntegrationState } from "../bindings/IntegrationState";
import type { IntegrationStatus } from "../bindings/IntegrationStatus";
import { errorMessage } from "../ipc/backend";
import { useBackend } from "../ipc/BackendContext";
import { useOfficeStore } from "../state/store";

const STATE_LABEL: Record<IntegrationState, string> = {
  notInstalled: "Not installed",
  installed: "Installed",
  needsRepair: "Needs repair",
  needsUserAction: "Needs your action",
  unsupported: "Not available",
  error: "Cannot read config",
};

const STATE_CLASS: Record<IntegrationState, string> = {
  notInstalled: "unknown",
  installed: "ok",
  needsRepair: "bad",
  needsUserAction: "bad",
  unsupported: "unknown",
  error: "bad",
};

/** Install / repair / uninstall hooks for one provider. Writes to the user's
 *  provider config only after an explicit confirmation. */
export function IntegrationControls({
  provider,
  name,
  status,
}: {
  provider: string;
  name: string;
  status: IntegrationStatus;
}) {
  const backend = useBackend();
  const pushToast = useOfficeStore((s) => s.pushToast);
  const [current, setCurrent] = useState(status);
  const [busy, setBusy] = useState(false);
  const [confirming, setConfirming] = useState<IntegrationAction | null>(null);

  useEffect(() => setCurrent(status), [status]);

  const run = async (action: IntegrationAction) => {
    setConfirming(null);
    setBusy(true);
    try {
      const next = await backend.integrationAction(provider, action);
      setCurrent(next);
      if (action !== "status") pushToast("info", `${name} hooks: ${STATE_LABEL[next.state].toLowerCase()}`);
    } catch (error) {
      pushToast("error", `${name} integration: ${errorMessage(error)}`);
    } finally {
      setBusy(false);
    }
  };

  const state = current.state;
  const where = current.configPath ?? "the provider's settings file";

  return (
    <div className="integration">
      <span className={`status ${STATE_CLASS[state]}`}>{STATE_LABEL[state]}</span>
      {current.details.map((d) => (
        <div key={d} className="muted small">
          {d}
        </div>
      ))}
      {current.configPath && <div className="mono small">{current.configPath}</div>}

      {state !== "unsupported" && confirming === null && (
        <div className="button-row wrap">
          {state === "notInstalled" && (
            <button className="btn small primary" disabled={busy} onClick={() => setConfirming("install")}>
              Install hooks
            </button>
          )}
          {state === "needsRepair" && (
            <button className="btn small primary" disabled={busy} onClick={() => setConfirming("repair")}>
              Repair
            </button>
          )}
          {(state === "installed" || state === "needsRepair" || state === "needsUserAction") && (
            <button className="btn small" disabled={busy} onClick={() => setConfirming("uninstall")}>
              Uninstall
            </button>
          )}
          <button className="btn small" disabled={busy} onClick={() => run("status")}>
            Check
          </button>
        </div>
      )}

      {confirming && (
        <div className="notice subtle">
          {confirming === "uninstall"
            ? `Remove the entries Agent Office added to ${where}. Everything else in the file stays as it is.`
            : `Add Agent Office hooks to ${where}. Your settings, plugins and hooks are kept, and a backup is saved next to the file first.`}
          <div className="button-row">
            <button className="btn small primary" onClick={() => run(confirming)}>
              Confirm
            </button>
            <button className="btn small" onClick={() => setConfirming(null)}>
              Cancel
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
