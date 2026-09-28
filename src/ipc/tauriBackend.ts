import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { AgentEvent } from "../bindings/AgentEvent";
import type { AppInfo } from "../bindings/AppInfo";
import type { DiagnosticsReport } from "../bindings/DiagnosticsReport";
import type { ExternalSessionInfo } from "../bindings/ExternalSessionInfo";
import type { InitialState } from "../bindings/InitialState";
import type { IntegrationStatus } from "../bindings/IntegrationStatus";
import type { LogTail } from "../bindings/LogTail";
import type { ManagedProcessInfo } from "../bindings/ManagedProcessInfo";
import type { Preferences } from "../bindings/Preferences";
import type { Project } from "../bindings/Project";
import type { RepositoriesReport } from "../bindings/RepositoriesReport";
import type { SessionHandle } from "../bindings/SessionHandle";
import type { UiBatch } from "../bindings/UiBatch";
import type { Backend } from "./backend";

export function createTauriBackend(): Backend {
  return {
    kind: "desktop",
    appInfo: () => invoke<AppInfo>("app_info"),
    async subscribe(onBatch) {
      const channel = new Channel<UiBatch>();
      channel.onmessage = onBatch;
      return invoke<InitialState>("subscribe", { channel });
    },
    runDiagnostics: () => invoke<DiagnosticsReport>("run_diagnostics"),
    listProjects: () => invoke<Project[]>("list_projects"),
    addProject: (project) => invoke<Project>("add_project", { project }),
    updateProject: (project) => invoke<void>("update_project", { project }),
    removeProject: (id) => invoke<boolean>("remove_project", { id }),
    async pickFolder() {
      const selected = await open({ directory: true, multiple: false, title: "Choose the project folder" });
      return typeof selected === "string" ? selected : null;
    },
    launchSession: (provider, request) => invoke<SessionHandle>("launch_session", { provider, request }),
    startDemoOffice: () => invoke<SessionHandle[]>("start_demo_office"),
    stopSession: (provider, sessionId, force) => invoke<void>("stop_session", { provider, sessionId, force }),
    restartSession: (provider, sessionId) => invoke<SessionHandle>("restart_session", { provider, sessionId }),
    openTerminal: (provider, sessionId) => invoke<void>("open_terminal", { provider, sessionId }),
    listProcesses: () => invoke<ManagedProcessInfo[]>("list_processes"),
    listRepositories: (refresh) => invoke<RepositoriesReport>("list_repositories", { refresh }),
    openFolder: (folder) => invoke<void>("open_folder", { folder }),
    revealFile: (folder, path) => invoke<void>("reveal_file", { folder, path }),
    onOpenAgent: (handler) => listen<string>("open-agent", (event) => handler(event.payload)),
    testNotification: () => invoke<void>("test_notification"),
    readLogs: (kind, maxLines, minLevel) => invoke<LogTail>("read_logs", { kind, maxLines, minLevel }),
    openLogFolder: () => invoke<void>("open_log_folder"),
    async exportDiagnostics() {
      const stamp = new Date().toISOString().slice(0, 10);
      const path = await save({
        title: "Save the diagnostics report",
        defaultPath: `agent-office-diagnostics-${stamp}.json`,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!path) return false;
      await invoke<void>("export_diagnostics", { path });
      return true;
    },
    sendPrompt: (provider, sessionId, prompt) => invoke<void>("send_prompt", { provider, sessionId, prompt }),
    resolvePermission: (provider, sessionId, requestId, decision) =>
      invoke<void>("resolve_permission", { provider, sessionId, requestId, decision }),
    recentEvents: (sessionKey, limit) => invoke<AgentEvent[]>("recent_events", { sessionKey, limit }),
    getPreferences: () => invoke<Preferences>("get_preferences"),
    setPreferences: (preferences) => invoke<Preferences>("set_preferences", { preferences }),
    integrationAction: (provider, action) => invoke<IntegrationStatus>("integration_action", { provider, action }),
    listExternalSessions: (provider) => invoke<ExternalSessionInfo[]>("list_external_sessions", { provider }),
  };
}
