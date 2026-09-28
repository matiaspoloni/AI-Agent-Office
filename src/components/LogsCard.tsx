import { useCallback, useEffect, useState } from "react";
import type { LogKind } from "../bindings/LogKind";
import type { LogLevel } from "../bindings/LogLevel";
import type { LogTail } from "../bindings/LogTail";
import { errorMessage } from "../ipc/backend";
import { useBackend } from "../ipc/BackendContext";
import { useOfficeStore } from "../state/store";
import { VirtualList } from "./VirtualList";

const LEVELS: { value: LogLevel; label: string }[] = [
  { value: "DEBUG", label: "Everything" },
  { value: "INFO", label: "Information and up" },
  { value: "WARN", label: "Warnings and errors" },
  { value: "ERROR", label: "Errors only" },
];

const TONE: Record<LogLevel, string> = { TRACE: "muted", DEBUG: "muted", INFO: "", WARN: "warn", ERROR: "bad" };

/** Agent Office's own logs: the application log and the provider log. */
export function LogsCard() {
  const backend = useBackend();
  const pushToast = useOfficeStore((s) => s.pushToast);
  const [kind, setKind] = useState<LogKind>("app");
  const [level, setLevel] = useState<LogLevel>("INFO");
  const [tail, setTail] = useState<LogTail | null>(null);

  const refresh = useCallback(async () => {
    try {
      setTail(await backend.readLogs(kind, 500, level));
    } catch (error) {
      pushToast("error", `Could not read the log: ${errorMessage(error)}`);
    }
  }, [backend, kind, level, pushToast]);

  useEffect(() => {
    void refresh();
    const id = window.setInterval(() => void refresh(), 10_000);
    return () => window.clearInterval(id);
  }, [refresh]);

  return (
    <section className="card">
      <div className="card-title-row">
        <h3>Logs</h3>
        <div className="button-row">
          <select value={kind} onChange={(e) => setKind(e.target.value as LogKind)} aria-label="Log">
            <option value="app">Application</option>
            <option value="providers">Providers (Claude, Codex, Cursor)</option>
          </select>
          <select value={level} onChange={(e) => setLevel(e.target.value as LogLevel)} aria-label="Level">
            {LEVELS.map((l) => (
              <option key={l.value} value={l.value}>
                {l.label}
              </option>
            ))}
          </select>
          <button className="btn small" onClick={() => void refresh()}>
            Refresh
          </button>
          <button
            className="btn small"
            onClick={() =>
              backend.openLogFolder().catch((e) => pushToast("error", `Open log folder: ${errorMessage(e)}`))
            }
          >
            Open log folder
          </button>
        </div>
      </div>
      <p className="small muted">
        {tail?.file ? (
          <>
            <span className="mono">{tail.file}</span> — the last {tail.lines.length} lines
            {tail.truncated ? " (older lines are in the file)" : ""}. More detail: start Agent Office with{" "}
            <code>AGENT_OFFICE_LOG=debug</code>.
          </>
        ) : (
          "Nothing logged yet."
        )}
      </p>
      {tail && tail.lines.length > 0 && (
        <VirtualList
          className="log"
          items={tail.lines}
          rowHeight={20}
          height={260}
          render={(line) => (
            <div className={`log-row ${TONE[line.level]}`} title={line.message}>
              <span className="log-time">{line.at.replace("T", " ").slice(0, 19)}</span>
              <span className="log-level">{line.level}</span>
              <span className="log-text">
                {line.target && <span className="muted">{line.target}: </span>}
                {line.message.split("\n")[0]}
              </span>
            </div>
          )}
        />
      )}
    </section>
  );
}
