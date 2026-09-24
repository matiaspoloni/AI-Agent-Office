import { beforeEach, describe, expect, it } from "vitest";
import type { UiBatch } from "../bindings/UiBatch";
import { shiftBatch } from "../ipc/timeshift";
import recording from "../ipc/preview-recording.json";
import { actionAvailability } from "./capabilities";
import { MAX_EVENTS_PER_SESSION, useOfficeStore } from "./store";

const frames = (recording as unknown as { frames: { at: number; batch: UiBatch }[] }).frames;
const providers = (recording as unknown as { providers: Parameters<typeof actionAvailability>[0][] }).providers;

beforeEach(() => {
  useOfficeStore.setState({ sessions: {}, agents: {}, eventsBySession: {}, selectedAgentKey: null, skippedEvents: 0 });
});

describe("store", () => {
  it("replaying the recorded demo builds a consistent office", () => {
    for (const f of frames) useOfficeStore.getState().applyBatch(f.batch);
    const { sessions, agents, eventsBySession } = useOfficeStore.getState();
    expect(Object.keys(sessions).length).toBe(4);
    for (const a of Object.values(agents)) expect(sessions[a.sessionKey]).toBeDefined();
    for (const list of Object.values(eventsBySession)) expect(list.length).toBeLessThanOrEqual(MAX_EVENTS_PER_SESSION);
  });

  it("removes sessions and clears a dangling selection", () => {
    const first = frames[0].batch;
    useOfficeStore.getState().applyBatch(first);
    const agentKey = first.agents[0].key;
    useOfficeStore.getState().select(agentKey);
    useOfficeStore.getState().applyBatch({
      seq: 999,
      sessions: [],
      agents: [],
      removedSessions: [first.agents[0].sessionKey],
      removedAgents: [agentKey],
      events: [],
      skippedEvents: 0,
    });
    expect(useOfficeStore.getState().agents[agentKey]).toBeUndefined();
    expect(useOfficeStore.getState().selectedAgentKey).toBeNull();
  });
});

describe("timeshift", () => {
  it("moves every timestamp by the same delta", () => {
    const batch = frames.find((f) => f.batch.agents.length && f.batch.events.length)!.batch;
    const shifted = shiftBatch(batch, 1000);
    expect(shifted.events[0].timestamp).toBe(batch.events[0].timestamp + 1000);
    expect(shifted.agents[0].activitySince).toBe(batch.agents[0].activitySince + 1000);
    expect(shifted.sessions[0]?.startedAt).toBe(batch.sessions[0]?.startedAt + 1000);
  });
});

describe("capability gating", () => {
  const byId = Object.fromEntries(providers.map((p) => [p!.descriptor.id, p]));

  it("never enables actions a provider does not support", () => {
    const claudeExternalPrompt = actionAvailability(byId.claude, "external", "sendPrompt");
    expect(claudeExternalPrompt.enabled).toBe(false);
    expect(claudeExternalPrompt.reason).toMatch(/does not support/);
  });

  it("does not enable supported-but-unimplemented actions", () => {
    const claudeManagedLaunch = actionAvailability(byId.claude, "managed", "launch");
    expect(claudeManagedLaunch.support).toBe("supported");
    expect(claudeManagedLaunch.enabled).toBe(false);
    expect(claudeManagedLaunch.reason).toMatch(/not implemented yet/);
  });

  it("enables implemented demo actions", () => {
    expect(actionAvailability(byId.demo, "managed", "launch").enabled).toBe(true);
    expect(actionAvailability(byId.demo, "managed", "permissions").enabled).toBe(true);
  });
});

describe("formatUsage", () => {
  it("never invents numbers and shows the context fill when reported", async () => {
    const { formatUsage } = await import("./format");
    expect(formatUsage(undefined, false)).toBe("Unavailable");
    expect(formatUsage(undefined, true)).toBe("Not reported yet");
    expect(formatUsage({ costIsEstimate: false }, true)).toBe("Not reported yet");
    expect(formatUsage({ contextTokens: 53000, contextWindow: 200000, costIsEstimate: false }, true)).toMatch(/^context 53/);
    const both = formatUsage({ inputTokens: 1000, outputTokens: 20, contextTokens: 5000, costIsEstimate: false }, true);
    expect(both).toContain("in ");
    expect(both).toContain("context ");
  });
});
