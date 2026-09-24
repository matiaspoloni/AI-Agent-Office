import { describe, expect, it } from "vitest";
import type { AgentEvent } from "../bindings/AgentEvent";
import type { AgentState } from "../bindings/AgentState";
import type { UiBatch } from "../bindings/UiBatch";
import { useOfficeStore } from "./store";

function agent(i: number): AgentState {
  return {
    key: `claude:s${i % 20}:a${i}`,
    sessionKey: `claude:s${i % 20}`,
    provider: "claude",
    sessionId: `s${i % 20}`,
    agentId: `a${i}`,
    isMain: i < 20,
    name: `agent ${i}`,
    activity: "CODING",
    activitySince: 0,
    runningTools: [],
    toolCalls: i,
    ended: false,
  };
}

function event(i: number): AgentEvent {
  return {
    eventId: `e${i}`,
    timestamp: 1_800_000_000_000 + i,
    provider: "claude",
    sessionId: `s${i % 20}`,
    agentId: `a${i % 70}`,
    source: "hook",
    type: "command.output",
    payload: { commandId: "c", stream: "stdout", chunk: `line ${i}` },
  };
}

describe("store performance", () => {
  it("absorbs 10,000 events in 100 ms batches without stalling", () => {
    const batches: UiBatch[] = [];
    for (let b = 0; b < 40; b++) {
      batches.push({
        seq: b,
        sessions: [],
        agents: Array.from({ length: 70 }, (_, i) => agent(i)),
        removedSessions: [],
        removedAgents: [],
        events: Array.from({ length: 250 }, (_, i) => event(b * 250 + i)),
        skippedEvents: 0,
      });
    }
    const started = performance.now();
    for (const batch of batches) useOfficeStore.getState().applyBatch(batch);
    const elapsed = performance.now() - started;
    const state = useOfficeStore.getState();
    expect(Object.keys(state.agents)).toHaveLength(70);
    for (const list of Object.values(state.eventsBySession)) expect(list.length).toBeLessThanOrEqual(400);
    // 40 batches ≈ 4 s of live traffic must be absorbed in well under a second.
    expect(elapsed).toBeLessThan(1000);
  });
});
