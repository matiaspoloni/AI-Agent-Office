// Browser preview backend (`npm run dev:web`). Replays a recording produced
// by the Rust demo provider on a virtual clock
// (`npm run preview:record`), so the UI can be built and tested without the
// desktop shell. Everything shown is simulated and labelled as such.

import type { InitialState } from "../bindings/InitialState";
import type { Preferences } from "../bindings/Preferences";
import type { Project } from "../bindings/Project";
import type { ProviderInfo } from "../bindings/ProviderInfo";
import type { UiBatch } from "../bindings/UiBatch";
import type { Backend } from "./backend";
import { stressRequested } from "./stressFlag";
import { shiftBatch } from "./timeshift";

interface PreviewRecording {
  providers: ProviderInfo[];
  baseMs: number;
  durationMs: number;
  frames: { at: number; batch: UiBatch }[];
}

const PROJECTS_KEY = "agent-office.preview.projects";
const STRESS_TICK_MS = 400;

const LOOP_GAP_MS = 4_000;
const PREVIEW_ONLY =
  "Not available in the browser preview. Run the desktop app (npm run dev) to control agents.";

function loadProjects(): Project[] {
  try {
    const raw = window.localStorage.getItem(PROJECTS_KEY);
    return raw ? (JSON.parse(raw) as Project[]) : [];
  } catch {
    return [];
  }
}

function saveProjects(projects: Project[]) {
  try {
    window.localStorage.setItem(PROJECTS_KEY, JSON.stringify(projects));
  } catch {
    // Storage can be unavailable (private mode); projects then live in memory only.
  }
}

export async function createPreviewBackend(): Promise<Backend> {
  const recording = (await import("./preview-recording.json")).default as unknown as PreviewRecording;
  let projects = loadProjects();
  let timers: number[] = [];
  let listener: ((batch: UiBatch) => void) | null = null;

  const playLoop = () => {
    const loopStart = Date.now();
    const delta = loopStart - recording.baseMs;
    timers = recording.frames.map((frame) =>
      window.setTimeout(() => listener?.(shiftBatch(frame.batch, delta)), frame.at),
    );
    timers.push(window.setTimeout(playLoop, recording.durationMs + LOOP_GAP_MS));
  };

  const playStress = async () => {
    const { StressOffice } = await import("./stress");
    const office = new StressOffice();
    listener?.(office.start(Date.now()));
    timers.push(window.setInterval(() => listener?.(office.tick(Date.now())), STRESS_TICK_MS));
  };

  return {
    kind: "preview",
    appInfo: async () => null,
    async subscribe(onBatch) {
      listener = onBatch;
      timers.forEach((t) => window.clearTimeout(t));
      timers = [];
      if (stressRequested()) void playStress();
      else playLoop();
      const initial: InitialState = { snapshot: { sessions: [], agents: [] }, providers: recording.providers };
      return initial;
    },
    runDiagnostics: async () => null,
    listProjects: async () => projects,
    async addProject(project) {
      if (!project.name.trim() || !project.path.trim()) throw new Error("Project name and path are required");
      if (projects.some((p) => p.path === project.path)) throw new Error(`A project already uses ${project.path}`);
      const now = Date.now();
      const created: Project = {
        id: crypto.randomUUID(),
        name: project.name.trim(),
        path: project.path.trim(),
        defaultProvider: project.defaultProvider,
        defaultModel: project.defaultModel,
        defaultBranch: project.defaultBranch,
        settings: {},
        createdAt: now,
        updatedAt: now,
      };
      projects = [...projects, created].sort((a, b) => a.name.localeCompare(b.name));
      saveProjects(projects);
      return created;
    },
    async updateProject(project) {
      projects = projects.map((p) => (p.id === project.id ? { ...project, updatedAt: Date.now() } : p));
      saveProjects(projects);
    },
    async removeProject(id) {
      const before = projects.length;
      projects = projects.filter((p) => p.id !== id);
      saveProjects(projects);
      return projects.length < before;
    },
    pickFolder: async () => null,
    launchSession: async () => {
      throw new Error(PREVIEW_ONLY);
    },
    startDemoOffice: async () => [],
    stopSession: async () => {
      throw new Error(PREVIEW_ONLY);
    },
    restartSession: async () => {
      throw new Error(PREVIEW_ONLY);
    },
    openTerminal: async () => {
      throw new Error(PREVIEW_ONLY);
    },
    // The preview starts no processes.
    listProcesses: async () => [],
    sendPrompt: async () => {
      throw new Error(PREVIEW_ONLY);
    },
    resolvePermission: async () => {
      throw new Error(PREVIEW_ONLY);
    },
    recentEvents: async () => [],
    getPreferences: async () => null,
    setPreferences: async (preferences: Preferences) => preferences,
    integrationAction: async () => {
      throw new Error(PREVIEW_ONLY);
    },
    listExternalSessions: async () => [],
  };
}
