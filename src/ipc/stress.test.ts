import { describe, expect, it } from "vitest";
import { STRESS_SESSIONS, STRESS_SUBAGENTS, StressOffice } from "./stress";

describe("stress office", () => {
  it("keeps 20 sessions with 50 subagents busy, deterministically", () => {
    const office = new StressOffice(7);
    const first = office.start(0);
    expect(first.sessions.length).toBe(STRESS_SESSIONS);
    expect(first.agents.filter((a) => a.isMain).length).toBe(STRESS_SESSIONS);
    expect(first.agents.filter((a) => !a.isMain).length).toBe(STRESS_SUBAGENTS);
    expect(first.agents.every((a) => a.provider === "demo")).toBe(true);
    const waits = (agents: { activity: string; pendingPermission?: unknown }[]) =>
      agents.every((a) => (a.activity === "WAITING_PERMISSION") === !!a.pendingPermission);
    expect(waits(first.agents)).toBe(true);
    let changed = 0;
    for (let t = 1; t <= 200; t++) {
      const batch = office.tick(t * 400);
      changed += batch.agents.length;
      expect(waits(batch.agents)).toBe(true);
    }
    expect(changed).toBeGreaterThan(200 * 8 * 0.9);
    expect(office.liveCounts()).toEqual({ leads: STRESS_SESSIONS, subagents: STRESS_SUBAGENTS });
    const again = new StressOffice(7);
    expect(again.start(0)).toEqual(first);
  });
});
