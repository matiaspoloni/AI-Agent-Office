// Office stress test for the browser preview (`npm run dev:web`, then open
// `/?stress`). Generates UI batches for 20 simulated sessions with 50
// subagents that keep changing activity, finishing and being replaced, so
// the office can be measured at the load the project targets. Everything
// uses the demo provider: it is labelled SIM like the rest of the preview.

import type { Activity } from "../bindings/Activity";
import type { AgentState } from "../bindings/AgentState";
import type { SessionState } from "../bindings/SessionState";
import type { UiBatch } from "../bindings/UiBatch";

export const STRESS_SESSIONS = 20;
export const STRESS_SUBAGENTS = 50;

const PROJECTS = ["Nalu", "Munder-Difflin", "Atlas", "Orion", "Kite", "Vega"];
const ROLES = ["Researcher", "Backend Agent", "QA Agent", "Explore", "Reviewer", "Planner"];
const ACTIVITIES: [Activity, number][] = [
  ["CODING", 30],
  ["READING", 20],
  ["THINKING", 18],
  ["RUNNING_COMMAND", 10],
  ["TESTING", 6],
  ["IDLE", 8],
  ["WAITING_PERMISSION", 3],
  ["WAITING_INPUT", 1],
  ["ERROR", 2],
];
const TOTAL_WEIGHT = ACTIVITIES.reduce((sum, [, w]) => sum + w, 0);

/** Small deterministic PRNG (mulberry32) so every run looks the same. */
export function prng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export class StressOffice {
  private readonly random: () => number;
  private readonly sessions = new Map<string, SessionState>();
  private readonly agents = new Map<string, AgentState>();
  private seq = 0;
  private spawned = 0;

  constructor(seed = 42) {
    this.random = prng(seed);
  }

  private pickActivity(): Activity {
    let roll = this.random() * TOTAL_WEIGHT;
    for (const [activity, weight] of ACTIVITIES) {
      roll -= weight;
      if (roll < 0) return activity;
    }
    return "CODING";
  }

  private sessionFor(i: number, now: number): SessionState {
    const project = PROJECTS[i % PROJECTS.length];
    const sessionId = `stress-${i}`;
    const key = `demo:${sessionId}`;
    return {
      key,
      provider: "demo",
      sessionId,
      mode: "managed",
      status: "active",
      mainAgentKey: `${key}:main`,
      agentKeys: [`${key}:main`],
      cwd: `C:\\Projects\\${project}`,
      title: `${project} · task ${i + 1}`,
      startedAt: now,
      lastEventAt: now,
      stats: { prompts: 1, toolCalls: 0, failedTools: 0, commands: 0, errors: 0, subagents: 0, commits: 0, filesChanged: [] },
    };
  }

  private agent(session: SessionState, key: string, isMain: boolean, name: string, now: number): AgentState {
    return {
      key,
      sessionKey: session.key,
      provider: "demo",
      sessionId: session.sessionId,
      agentId: key.split(":").pop() ?? key,
      isMain,
      parentKey: isMain ? undefined : session.mainAgentKey,
      name,
      agentType: isMain ? undefined : name,
      activity: this.pickActivity(),
      activitySince: now,
      currentAction: "Simulated work",
      runningTools: [],
      toolCalls: 0,
      ended: false,
    };
  }

  private spawnSubagent(now: number): AgentState {
    const n = this.spawned++;
    const session = [...this.sessions.values()][n % STRESS_SESSIONS];
    const key = `${session.key}:sub-${n}`;
    const sub = this.agent(session, key, false, ROLES[n % ROLES.length], now);
    this.agents.set(key, sub);
    // Batches hand objects to the UI store: never mutate one already sent.
    this.sessions.set(session.key, {
      ...session,
      agentKeys: [...session.agentKeys, key],
      stats: { ...session.stats, subagents: session.stats.subagents + 1 },
    });
    return sub;
  }

  /** The first batch: every session and subagent at once. */
  start(now: number): UiBatch {
    for (let i = 0; i < STRESS_SESSIONS; i++) {
      const session = this.sessionFor(i, now);
      this.sessions.set(session.key, session);
      const lead = this.agent(session, session.mainAgentKey, true, session.title ?? session.key, now);
      this.agents.set(lead.key, lead);
    }
    for (let i = 0; i < STRESS_SUBAGENTS; i++) this.spawnSubagent(now);
    return this.batch([...this.sessions.values()], [...this.agents.values()]);
  }

  /** One tick: some agents change activity; now and then a subagent finishes and a new one starts. */
  tick(now: number): UiBatch {
    const changed = new Map<string, AgentState>();
    const sessions = new Map<string, SessionState>();
    const live = [...this.agents.values()].filter((a) => !a.ended);
    for (let i = 0; i < 8; i++) {
      const a = live[Math.floor(this.random() * live.length)];
      const next = { ...a, activity: this.pickActivity(), activitySince: now, toolCalls: a.toolCalls + 1 };
      this.agents.set(a.key, next);
      changed.set(a.key, next);
    }
    if (this.random() < 0.35) {
      const subs = live.filter((a) => !a.isMain);
      const done = subs[Math.floor(this.random() * subs.length)];
      if (done) {
        const ended = { ...done, ended: true, endedAt: now, activity: "DONE" as Activity, activitySince: now };
        this.agents.set(done.key, ended);
        changed.set(done.key, ended);
        const fresh = this.spawnSubagent(now);
        changed.set(fresh.key, fresh);
        const session = this.sessions.get(fresh.sessionKey);
        if (session) sessions.set(session.key, session);
      }
    }
    // Forget agents that left long ago (the core does the same).
    const removed: string[] = [];
    for (const a of this.agents.values()) {
      if (a.ended && a.endedAt !== undefined && now - a.endedAt > 30_000) removed.push(a.key);
    }
    for (const key of removed) this.agents.delete(key);
    return this.batch([...sessions.values()], [...changed.values()], removed);
  }

  private batch(sessions: SessionState[], agents: AgentState[], removedAgents: string[] = []): UiBatch {
    return { seq: ++this.seq, sessions, agents, removedSessions: [], removedAgents, events: [], skippedEvents: 0 };
  }

  liveCounts(): { leads: number; subagents: number } {
    let leads = 0;
    let subagents = 0;
    for (const a of this.agents.values()) {
      if (a.ended) continue;
      if (a.isMain) leads++;
      else subagents++;
    }
    return { leads, subagents };
  }
}
