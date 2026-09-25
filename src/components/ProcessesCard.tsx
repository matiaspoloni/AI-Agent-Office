import { useCallback, useEffect, useState } from "react";
import type { ManagedProcessInfo } from "../bindings/ManagedProcessInfo";
import { errorMessage } from "../ipc/backend";
import { useBackend } from "../ipc/BackendContext";
import { formatDuration, formatTime } from "../state/format";

const REFRESH_MS = 5000;

function ended(p: ManagedProcessInfo): "ok" | "bad" {
  return p.status === "finished" || p.status.startsWith("stopped by Agent Office") ? "ok" : "bad";
}

/** The agent processes Agent Office started (and is allowed to stop). */
export function ProcessesCard() {
  const backend = useBackend();
  const [processes, setProcesses] = useState<ManagedProcessInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());

  const refresh = useCallback(async () => {
    try {
      setProcesses(await backend.listProcesses());
      setError(null);
    } catch (e) {
      setError(errorMessage(e));
    }
    setNow(Date.now());
  }, [backend]);

  useEffect(() => {
    void refresh();
    const id = window.setInterval(() => void refresh(), REFRESH_MS);
    return () => window.clearInterval(id);
  }, [refresh]);

  return (
    <section className="card">
      <div className="card-title-row">
        <h3>Processes started by Agent Office</h3>
        <button className="btn small" onClick={() => void refresh()}>
          Refresh
        </button>
      </div>
      <p className="small muted">
        Agent Office only stops processes it started itself (listed here), together with everything they started. Agents
        you run in your own terminals are never stopped.
      </p>
      {error && <p className="bad-text small">{error}</p>}
      {processes && processes.length === 0 && <p className="muted">None right now.</p>}
      {processes && processes.length > 0 && (
        <table className="table">
          <thead>
            <tr>
              <th>Agent</th>
              <th>PID</th>
              <th>Started</th>
              <th>Status</th>
              <th title="Processes alive in its tree: the agent plus what it started">Tree</th>
              <th>Last output</th>
            </tr>
          </thead>
          <tbody>
            {processes.map((p) => (
              <tr key={`${p.pid}-${p.startedAt}`}>
                <td>
                  {p.label}
                  <div className="mono small muted">{p.program}</div>
                </td>
                <td className="mono">{p.pid}</td>
                <td className="small">{formatTime(p.startedAt)}</td>
                <td>
                  <span className={`status ${p.running ? "ok" : ended(p)}`}>{p.running ? "running" : p.status}</span>
                </td>
                <td>{p.treeProcesses ?? "—"}</td>
                <td className="small">
                  {p.lastOutputAt != null ? `${formatDuration(Math.max(0, now - p.lastOutputAt))} ago` : "none yet"}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
