// The UI talks to the Rust core only through this interface.
// Desktop: Tauri commands + a streaming Channel. Browser preview: a replay
// of a recorded demo timeline (see previewBackend.ts).

import type { AgentEvent } from "../bindings/AgentEvent";
import type { AppInfo } from "../bindings/AppInfo";
import type { DiagnosticsReport } from "../bindings/DiagnosticsReport";
import type { ExternalSessionInfo } from "../bindings/ExternalSessionInfo";
import type { InitialState } from "../bindings/InitialState";
import type { IntegrationAction } from "../bindings/IntegrationAction";
import type { IntegrationStatus } from "../bindings/IntegrationStatus";
import type { LaunchRequest } from "../bindings/LaunchRequest";
import type { NewProject } from "../bindings/NewProject";
import type { PermissionDecision } from "../bindings/PermissionDecision";
import type { Preferences } from "../bindings/Preferences";
import type { Project } from "../bindings/Project";
import type { SessionHandle } from "../bindings/SessionHandle";
import type { UiBatch } from "../bindings/UiBatch";

export type BackendKind = "desktop" | "preview";

export interface Backend {
  kind: BackendKind;
  appInfo(): Promise<AppInfo | null>;
  subscribe(onBatch: (batch: UiBatch) => void): Promise<InitialState>;
  runDiagnostics(): Promise<DiagnosticsReport | null>;
  listProjects(): Promise<Project[]>;
  addProject(project: NewProject): Promise<Project>;
  updateProject(project: Project): Promise<void>;
  removeProject(id: string): Promise<boolean>;
  pickFolder(): Promise<string | null>;
  launchSession(provider: string, request: LaunchRequest): Promise<SessionHandle>;
  startDemoOffice(): Promise<SessionHandle[]>;
  stopSession(provider: string, sessionId: string, force: boolean): Promise<void>;
  /** Continue a managed session's conversation in a new process (stops it first if running). */
  restartSession(provider: string, sessionId: string): Promise<SessionHandle>;
  sendPrompt(provider: string, sessionId: string, prompt: string): Promise<void>;
  resolvePermission(
    provider: string,
    sessionId: string,
    requestId: string,
    decision: PermissionDecision,
  ): Promise<void>;
  recentEvents(sessionKey: string, limit?: number): Promise<AgentEvent[]>;
  getPreferences(): Promise<Preferences | null>;
  setPreferences(preferences: Preferences): Promise<Preferences>;
  /** Install / repair / uninstall / check a provider's hook integration. */
  integrationAction(provider: string, action: IntegrationAction): Promise<IntegrationStatus>;
  /** Sessions reported by the provider's official listing command. */
  listExternalSessions(provider: string): Promise<ExternalSessionInfo[]>;
}

export function isDesktop(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

let backendPromise: Promise<Backend> | null = null;

export function getBackend(): Promise<Backend> {
  if (!backendPromise) {
    backendPromise = isDesktop()
      ? import("./tauriBackend").then((m) => m.createTauriBackend())
      : import("./previewBackend").then((m) => m.createPreviewBackend());
  }
  return backendPromise;
}

/** Normalizes errors thrown by Tauri commands (plain strings) and JS errors. */
export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return JSON.stringify(error);
}
