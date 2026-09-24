import { Fragment, useEffect, useState } from "react";
import type { Capabilities } from "../bindings/Capabilities";
import type { DiagnosticsReport } from "../bindings/DiagnosticsReport";
import type { ExternalSessionInfo } from "../bindings/ExternalSessionInfo";
import { IntegrationControls } from "../components/IntegrationControls";
import { PreferencesCard } from "../components/PreferencesCard";
import { errorMessage } from "../ipc/backend";
import { useBackend } from "../ipc/BackendContext";
import { formatDuration, formatTime, SUPPORT_LABEL } from "../state/format";
import { useOfficeStore } from "../state/store";

function Status({ ok, children }: { ok: boolean | null; children: React.ReactNode }) {
  return <span className={`status ${ok === null ? "unknown" : ok ? "ok" : "bad"}`}>{children}</span>;
}

function bytes(n: number) {
  if (n > 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024).toFixed(0)} KB`;
}

export function DiagnosticsView() {
  const backend = useBackend();
  const pushToast = useOfficeStore((s) => s.pushToast);
  const [report, setReport] = useState<DiagnosticsReport | null>(null);
  const [running, setRunning] = useState(false);
  const [listed, setListed] = useState<{ provider: string; sessions: ExternalSessionInfo[] } | null>(null);

  const listSessions = async (provider: string) => {
    try {
      setListed({ provider, sessions: await backend.listExternalSessions(provider) });
    } catch (error) {
      pushToast("error", `Could not list sessions: ${errorMessage(error)}`);
    }
  };

  const run = async () => {
    setRunning(true);
    try {
      setReport(await backend.runDiagnostics());
    } catch (error) {
      pushToast("error", `Diagnostics failed: ${errorMessage(error)}`);
    } finally {
      setRunning(false);
    }
  };

  useEffect(() => {
    void run();
    // Run once when the screen opens; later runs are explicit.
  }, []);

  if (backend.kind === "preview") {
    return (
      <div className="card">
        <h3>Diagnostics</h3>
        <p>
          Diagnostics inspect your machine (installed CLIs, hook files, database) and are only available in the desktop
          app. Run <code>npm run dev</code>.
        </p>
      </div>
    );
  }

  return (
    <div className="diagnostics">
      <div className="button-row">
        <button className="btn primary" onClick={run} disabled={running}>
          {running ? "Running…" : "Run diagnostics"}
        </button>
        {report && <span className="muted small">Last run {formatTime(report.generatedAt)}</span>}
      </div>

      <PreferencesCard onSaved={run} />

      {report && (
        <>
          <section className="card">
            <h3>Providers</h3>
            <table className="table">
              <thead>
                <tr>
                  <th>Provider</th>
                  <th>Installed</th>
                  <th>Version</th>
                  <th>Executable</th>
                  <th>Hooks</th>
                  <th>Implemented in this build</th>
                </tr>
              </thead>
              <tbody>
                {report.providers.map(({ provider, installation, integration }) => {
                  const impl = provider.capabilities.implemented;
                  return (
                    <tr key={provider.descriptor.id}>
                      <td>
                        <span className="badge" style={{ background: provider.descriptor.accentColor }}>
                          {provider.descriptor.badge}
                        </span>{" "}
                        {provider.descriptor.displayName}
                      </td>
                      <td>
                        <Status ok={installation.installed}>{installation.installed ? "Yes" : "No"}</Status>
                        {installation.error && <div className="muted small">{installation.error}</div>}
                      </td>
                      <td>{installation.version ?? installation.versionOutput ?? "—"}</td>
                      <td className="mono small">{installation.executablePath ?? "—"}</td>
                      <td>
                        <IntegrationControls
                          provider={provider.descriptor.id}
                          name={provider.descriptor.displayName}
                          status={integration}
                        />
                      </td>
                      <td className="small">
                        detection {impl.detection ? "✔" : "✖"} · managed {impl.managedSessions ? "✔" : "✖"} · external{" "}
                        {impl.externalSessions ? "✔" : "✖"}
                        {impl.externalSessions &&
                          installation.installed &&
                          provider.capabilities.external.listSessions !== "unsupported" && (
                            <div>
                              <button className="btn small" onClick={() => listSessions(provider.descriptor.id)}>
                                List running sessions
                              </button>
                            </div>
                          )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </section>

          {listed && (
            <section className="card">
              <h3>
                Sessions reported by {report.providers.find((p) => p.provider.descriptor.id === listed.provider)?.provider.descriptor.displayName}
              </h3>
              {listed.sessions.length === 0 && <p className="muted">No running sessions.</p>}
              {listed.sessions.length > 0 && (
                <table className="table">
                  <thead>
                    <tr>
                      <th>Session</th>
                      <th>Name</th>
                      <th>Folder</th>
                      <th>Status</th>
                      <th>PID</th>
                    </tr>
                  </thead>
                  <tbody>
                    {listed.sessions.map((s) => (
                      <tr key={s.sessionId}>
                        <td className="mono small">{s.sessionId}</td>
                        <td>{s.title ?? "—"}</td>
                        <td className="mono small">{s.cwd ?? "—"}</td>
                        <td>{s.status ?? "—"}</td>
                        <td>{s.pid ?? "—"}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </section>
          )}

          <section className="card">
            <h3>Hook bridge</h3>
            <p>
              <Status ok={report.hooks.listening}>{report.hooks.listening ? "Listening" : "Not listening"}</Status>
              {report.hooks.error && <span className="bad-text"> {report.hooks.error}</span>}
            </p>
            <p className="small">
              Local endpoint <span className="mono">{report.hooks.endpoint}</span> (named pipe / socket, never a network
              port)
            </p>
            <p className="small">
              Relay command <span className="mono">{report.hooks.relayCommand ?? "unavailable"}</span>{" "}
              {report.hooks.relayCommand && !report.hooks.relayExists && <span className="bad-text">(file missing)</span>}
            </p>
            {report.hooks.events.length === 0 ? (
              <p className="muted small">No hook events received since Agent Office started.</p>
            ) : (
              <table className="table">
                <thead>
                  <tr>
                    <th>Provider</th>
                    <th>Hook event</th>
                    <th>Received</th>
                    <th>Last</th>
                  </tr>
                </thead>
                <tbody>
                  {report.hooks.events.map((e) => (
                    <tr key={`${e.provider}/${e.event}`}>
                      <td>{e.provider}</td>
                      <td className="mono small">{e.event}</td>
                      <td>{e.count}</td>
                      <td>{formatTime(e.lastAt)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </section>

          <section className="card">
            <h3>Capabilities (as documented for each provider)</h3>
            <table className="table capabilities">
              <thead>
                <tr>
                  <th>Capability</th>
                  {report.providers.map((p) => (
                    <th key={p.provider.descriptor.id} colSpan={2}>
                      {p.provider.descriptor.displayName}
                    </th>
                  ))}
                </tr>
                <tr>
                  <th />
                  {report.providers.map((p) => (
                    <Fragment key={p.provider.descriptor.id}>
                      <th>managed</th>
                      <th>external</th>
                    </Fragment>
                  ))}
                </tr>
              </thead>
              <tbody>
                {(Object.keys(report.providers[0]?.provider.capabilities.managed ?? {}) as (keyof Capabilities)[]).map((cap) => (
                  <tr key={cap}>
                    <td>{cap}</td>
                    {report.providers.map((p) => (
                      <Fragment key={p.provider.descriptor.id}>
                        <td className={`support ${p.provider.capabilities.managed[cap]}`}>
                          {SUPPORT_LABEL[p.provider.capabilities.managed[cap]]}
                        </td>
                        <td className={`support ${p.provider.capabilities.external[cap]}`}>
                          {SUPPORT_LABEL[p.provider.capabilities.external[cap]]}
                        </td>
                      </Fragment>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </section>

          <div className="grid-2">
            <section className="card">
              <h3>Backend</h3>
              <p>
                <Status ok={report.backend.ok}>{report.backend.ok ? "Running" : "Problem"}</Status> · uptime{" "}
                {formatDuration(report.backend.uptimeMs)} · {report.platform} · v{report.appVersion}
              </p>
              <p className="small">
                Events ingested {report.backend.eventsIngested} · dropped {report.backend.eventsDropped} · duplicates{" "}
                {report.backend.duplicatesDropped} · rejected {report.backend.eventsRejected} · active sessions{" "}
                {report.backend.activeSessions}
              </p>
              <p className="small muted">
                Stored output limit {report.backend.maxOutputChars} chars · prompts{" "}
                {report.backend.storePrompts ? "stored" : "not stored"}
              </p>
            </section>
            <section className="card">
              <h3>Database</h3>
              <p>
                <Status ok={report.database.ok}>{report.database.ok ? "Healthy" : "Problem"}</Status>
                {report.database.error && <span className="bad-text"> {report.database.error}</span>}
              </p>
              {report.database.stats && (
                <p className="small">
                  schema v{report.database.stats.schemaVersion}/{report.database.stats.latestSchemaVersion} ·{" "}
                  {report.database.stats.sessions} sessions · {report.database.stats.events} events ·{" "}
                  {bytes(report.database.stats.sizeBytes)}
                </p>
              )}
            </section>
            <section className="card">
              <h3>Git</h3>
              <p>
                <Status ok={report.git.installed}>{report.git.installed ? "Detected" : "Not found"}</Status>{" "}
                {report.git.version && `v${report.git.version}`}
              </p>
              <p className="mono small">{report.git.executablePath ?? report.git.error}</p>
            </section>
            <section className="card">
              <h3>Provider errors</h3>
              {report.providerErrors.length === 0 && <p className="muted">None.</p>}
              {report.providerErrors.map((e, i) => (
                <p key={i} className="small">
                  {formatTime(e.at)} · {e.provider}/{e.component}: {e.message}
                </p>
              ))}
            </section>
          </div>

          <section className="card">
            <h3>Paths</h3>
            <table className="table">
              <tbody>
                {report.paths.map((p) => (
                  <tr key={p.label}>
                    <td>{p.label}</td>
                    <td className="mono small">{p.path}</td>
                    <td>
                      <Status ok={p.exists ? true : null}>{p.exists ? "exists" : "not present"}</Status>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>
        </>
      )}
    </div>
  );
}
