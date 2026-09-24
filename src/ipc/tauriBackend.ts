import { Channel, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { AgentEvent } from "../bindings/AgentEvent";
import type { AppInfo } from "../bindings/AppInfo";
import type { DiagnosticsReport } from "../bindings/DiagnosticsReport";
import type { ExternalSessionInfo } from "../bindings/ExternalSessionInfo";
import type { InitialState } from "../bindings/InitialState";
import type { IntegrationStatus } from "../bindings/IntegrationStatus";
import type { Preferences } from "../bindings/Preferences";
import type { Project } from "../bindings/Project";
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
