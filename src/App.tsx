import { useEffect, useState } from "react";
import { AgentPanel } from "./components/AgentPanel";
import { NewAgentDialog } from "./components/NewAgentDialog";
import { type Backend, errorMessage, getBackend } from "./ipc/backend";
import { BackendContext } from "./ipc/BackendContext";
import { OfficeView } from "./office/OfficeView";
import { type View, useOfficeStore } from "./state/store";
import { CommandCenter } from "./views/CommandCenter";
import { DiagnosticsView } from "./views/DiagnosticsView";
import { ProjectsView } from "./views/ProjectsView";

const TABS: { id: View; label: string }[] = [
  { id: "office", label: "Office" },
  { id: "command", label: "Command Center" },
  { id: "projects", label: "Projects" },
  { id: "diagnostics", label: "Diagnostics" },
];

function Toasts() {
  const toasts = useOfficeStore((s) => s.toasts);
  const dismiss = useOfficeStore((s) => s.dismissToast);
  return (
    <div className="toasts" role="status">
      {toasts.map((t) => (
        <button key={t.id} className={`toast ${t.kind}`} onClick={() => dismiss(t.id)}>
          {t.text}
        </button>
      ))}
    </div>
  );
}

function HeaderStats() {
  const agents = useOfficeStore((s) => s.agents);
  let active = 0;
  let waiting = 0;
  let errors = 0;
  for (const a of Object.values(agents)) {
    if (a.ended) continue;
    active++;
    if (a.pendingPermission) waiting++;
    if (a.activity === "ERROR") errors++;
  }
  return (
    <div className="header-stats">
      <span>{active} active</span>
      <span className={waiting ? "warn-text" : ""}>{waiting} waiting</span>
      <span className={errors ? "bad-text" : ""}>{errors} errors</span>
    </div>
  );
}

export default function App() {
  const [backend, setBackend] = useState<Backend | null>(null);
  const [fatal, setFatal] = useState<string | null>(null);
  const [newAgentOpen, setNewAgentOpen] = useState(false);
  const view = useOfficeStore((s) => s.view);
  const setView = useOfficeStore((s) => s.setView);
  const selected = useOfficeStore((s) => s.selectedAgentKey);
  const pushToast = useOfficeStore((s) => s.pushToast);
  const hasAgents = useOfficeStore((s) => Object.keys(s.agents).length > 0);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const b = await getBackend();
        const store = useOfficeStore.getState();
        const initial = await b.subscribe((batch) => useOfficeStore.getState().applyBatch(batch));
        if (cancelled) return;
        store.applyInitial(initial, b.kind);
        store.setProjects(await b.listProjects());
        setBackend(b);
      } catch (error) {
        setFatal(errorMessage(error));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  if (fatal) {
    return (
      <div className="fatal">
        <h2>Agent Office could not start</h2>
        <p>{fatal}</p>
      </div>
    );
  }
  if (!backend) return <div className="loading">Opening the office…</div>;

  const startDemo = async () => {
    try {
      const sessions = await backend.startDemoOffice();
      if (sessions.length) pushToast("info", `Started ${sessions.length} simulated demo agents`);
    } catch (error) {
      pushToast("error", errorMessage(error));
    }
  };

  return (
    <BackendContext.Provider value={backend}>
      <div className="app">
        <header className="topbar">
          <div className="brand">
            <img src="/favicon.png" alt="" width={20} height={20} />
            <strong>Agent Office</strong>
          </div>
          <nav className="tabs" aria-label="Views">
            {TABS.map((t) => (
              <button key={t.id} className={`tab ${view === t.id ? "active" : ""}`} onClick={() => setView(t.id)}>
                {t.label}
              </button>
            ))}
          </nav>
          <HeaderStats />
          <div className="topbar-actions">
            {backend.kind === "desktop" && (
              <button className="btn" onClick={startDemo} title="Spawn simulated agents to explore the office">
                Demo office
              </button>
            )}
            <button className="btn primary" onClick={() => setNewAgentOpen(true)}>
              + New agent
            </button>
          </div>
        </header>

        {backend.kind === "preview" && (
          <div className="banner">
            Browser preview — replaying a recorded, simulated demo. Run <code>npm run dev</code> for the desktop app with
            real providers.
          </div>
        )}

        <main className={`main ${selected && (view === "office" || view === "command") ? "with-panel" : ""}`}>
          <div className="view">
            {view === "office" && (
              <>
                <OfficeView />
                {!hasAgents && (
                  <div className="empty-office">
                    <p>The office is empty.</p>
                    <p className="muted small">
                      Launch an agent with <strong>+ New agent</strong>
                      {backend.kind === "desktop" ? (
                        <>
                          , or press <strong>Demo office</strong> to see simulated agents at work.
                        </>
                      ) : (
                        "."
                      )}
                    </p>
                  </div>
                )}
              </>
            )}
            {view === "command" && <CommandCenter />}
            {view === "projects" && <ProjectsView />}
            {view === "diagnostics" && <DiagnosticsView />}
          </div>
          {selected && (view === "office" || view === "command") && <AgentPanel />}
        </main>
        {newAgentOpen && <NewAgentDialog onClose={() => setNewAgentOpen(false)} />}
        <Toasts />
      </div>
    </BackendContext.Provider>
  );
}
