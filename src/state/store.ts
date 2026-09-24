// Normalized UI state. The Rust core is the source of truth: it sends full
// SessionState/AgentState objects for whatever changed, so this store only
// merges by key — there is no event reducer duplicated in TypeScript.

import { create } from "zustand";
import type { AgentEvent } from "../bindings/AgentEvent";
import type { AgentState } from "../bindings/AgentState";
import type { InitialState } from "../bindings/InitialState";
import type { Project } from "../bindings/Project";
import type { ProviderInfo } from "../bindings/ProviderInfo";
import type { SessionState } from "../bindings/SessionState";
import type { UiBatch } from "../bindings/UiBatch";
import type { BackendKind } from "../ipc/backend";

export const MAX_EVENTS_PER_SESSION = 400;

export type View = "office" | "command" | "projects" | "diagnostics";

export interface Toast {
  id: number;
  kind: "info" | "error";
  text: string;
}

export interface OfficeState {
  ready: boolean;
  backendKind: BackendKind | null;
  providers: Record<string, ProviderInfo>;
  providerOrder: string[];
  sessions: Record<string, SessionState>;
  agents: Record<string, AgentState>;
  eventsBySession: Record<string, AgentEvent[]>;
  projects: Project[];
  selectedAgentKey: string | null;
  view: View;
  lastSeq: number;
  skippedEvents: number;
  toasts: Toast[];
  applyInitial(initial: InitialState, backendKind: BackendKind): void;
  applyBatch(batch: UiBatch): void;
  setProjects(projects: Project[]): void;
  select(agentKey: string | null): void;
  setView(view: View): void;
  pushToast(kind: Toast["kind"], text: string): void;
  dismissToast(id: number): void;
}

let toastSeq = 0;

function sessionKeyOf(e: AgentEvent): string {
  return `${e.provider}:${e.sessionId}`;
}

export const useOfficeStore = create<OfficeState>()((set) => ({
  ready: false,
  backendKind: null,
  providers: {},
  providerOrder: [],
  sessions: {},
  agents: {},
  eventsBySession: {},
  projects: [],
  selectedAgentKey: null,
  view: "office",
  lastSeq: 0,
  skippedEvents: 0,
  toasts: [],

  applyInitial(initial, backendKind) {
    const providers: Record<string, ProviderInfo> = {};
    for (const p of initial.providers) providers[p.descriptor.id] = p;
    const sessions: Record<string, SessionState> = {};
    for (const s of initial.snapshot.sessions) sessions[s.key] = s;
    const agents: Record<string, AgentState> = {};
    for (const a of initial.snapshot.agents) agents[a.key] = a;
    set({
      ready: true,
      backendKind,
      providers,
      providerOrder: initial.providers.map((p) => p.descriptor.id),
      sessions,
      agents,
    });
  },

  applyBatch(batch) {
    set((state) => {
      const sessions = batch.sessions.length || batch.removedSessions.length ? { ...state.sessions } : state.sessions;
      for (const s of batch.sessions) sessions[s.key] = s;
      for (const key of batch.removedSessions) delete sessions[key];

      const agents = batch.agents.length || batch.removedAgents.length ? { ...state.agents } : state.agents;
      for (const a of batch.agents) agents[a.key] = a;
      for (const key of batch.removedAgents) delete agents[key];

      let eventsBySession = state.eventsBySession;
      if (batch.events.length || batch.removedSessions.length) {
        eventsBySession = { ...eventsBySession };
        for (const e of batch.events) {
          const key = sessionKeyOf(e);
          const list = eventsBySession[key] ? [...eventsBySession[key], e] : [e];
          eventsBySession[key] = list.length > MAX_EVENTS_PER_SESSION ? list.slice(-MAX_EVENTS_PER_SESSION) : list;
        }
        for (const key of batch.removedSessions) delete eventsBySession[key];
      }

      const selectedAgentKey =
        state.selectedAgentKey && !agents[state.selectedAgentKey] ? null : state.selectedAgentKey;

      return {
        sessions,
        agents,
        eventsBySession,
        selectedAgentKey,
        lastSeq: Number(batch.seq),
        skippedEvents: state.skippedEvents + batch.skippedEvents,
      };
    });
  },

  setProjects(projects) {
    set({ projects });
  },

  select(agentKey) {
    set({ selectedAgentKey: agentKey });
  },

  setView(view) {
    set({ view });
  },

  pushToast(kind, text) {
    const id = ++toastSeq;
    set((state) => ({ toasts: [...state.toasts.slice(-3), { id, kind, text }] }));
    window.setTimeout(() => useOfficeStore.getState().dismissToast(id), kind === "error" ? 7000 : 4000);
  },

  dismissToast(id) {
    set((state) => ({ toasts: state.toasts.filter((t) => t.id !== id) }));
  },
}));
