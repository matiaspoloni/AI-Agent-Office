import { describe, expect, it } from "vitest";
import type { AgentState } from "../bindings/AgentState";
import { buildGrid, DEFAULT_LAYOUT, findPath } from "./layout";
import { DONE_CELEBRATION_MS, OfficeScene, zoneFor } from "./scene";

function agent(key: string, partial: Partial<AgentState> = {}): AgentState {
  return {
    key,
    sessionKey: "demo:s",
    provider: "demo",
    sessionId: "s",
    agentId: key,
    isMain: true,
    name: key,
    activity: "CODING",
    activitySince: 0,
    runningTools: [],
    toolCalls: 0,
    ended: false,
    ...partial,
  };
}

function settle(scene: OfficeScene) {
  for (let i = 0; i < 2000 && scene.step(100); i++);
}

describe("layout", () => {
  it("every seat is reachable from the entrance", () => {
    const grid = buildGrid(DEFAULT_LAYOUT);
    for (const seat of DEFAULT_LAYOUT.seats) {
      const path = findPath(grid, DEFAULT_LAYOUT.entrance, seat);
      expect(path.length, `seat ${seat.id}`).toBeGreaterThan(0);
    }
  });

  it("paths never cross walls or furniture", () => {
    const grid = buildGrid(DEFAULT_LAYOUT);
    const target = DEFAULT_LAYOUT.seats.find((s) => s.zone === "qa")!;
    const path = findPath(grid, DEFAULT_LAYOUT.entrance, target);
    for (const step of path) expect(grid.walkable[step.y * grid.width + step.x]).toBe(true);
  });
});

describe("zoneFor", () => {
  it("maps activities to office zones", () => {
    expect(zoneFor({ activity: "CODING", isMain: true, ended: false })).toBe("desk");
    expect(zoneFor({ activity: "READING", isMain: false, ended: false })).toBe("meeting");
    expect(zoneFor({ activity: "RUNNING_COMMAND", isMain: true, ended: false })).toBe("terminal");
    expect(zoneFor({ activity: "TESTING", isMain: true, ended: false })).toBe("qa");
    expect(zoneFor({ activity: "IDLE", isMain: true, ended: false })).toBe("lounge");
    expect(zoneFor({ activity: "WAITING_PERMISSION", isMain: true, ended: false })).toBe("desk");
    expect(zoneFor({ activity: "CODING", isMain: true, ended: true })).toBe("exit");
  });
});

describe("OfficeScene", () => {
  it("places agents instantly on first sync and gives each a distinct seat", () => {
    const scene = new OfficeScene(DEFAULT_LAYOUT);
    scene.sync([agent("a"), agent("b"), agent("c", { activity: "TESTING" })], 0);
    const seats = [...scene.entities.values()].map((e) => e.seatId);
    expect(new Set(seats).size).toBe(3);
    expect([...scene.entities.values()].every((e) => e.path.length === 0)).toBe(true);
  });

  it("new agents walk in and return to the same desk after a break", () => {
    const scene = new OfficeScene(DEFAULT_LAYOUT);
    scene.sync([], 0);
    scene.sync([agent("a")], 0);
    const entity = scene.entities.get("a")!;
    expect(entity.path.length).toBeGreaterThan(0);
    settle(scene);
    const desk = entity.seatId;
    expect(scene.seat(desk)?.zone).toBe("desk");

    scene.sync([agent("a", { activity: "IDLE" })], 1);
    settle(scene);
    expect(scene.seat(entity.seatId)?.zone).toBe("lounge");

    // Someone else cannot take the reserved desk meanwhile.
    scene.sync([agent("a", { activity: "IDLE" }), agent("b")], 2);
    expect(scene.entities.get("b")!.seatId).not.toBe(desk);

    scene.sync([agent("a"), agent("b")], 3);
    settle(scene);
    expect(entity.seatId).toBe(desk);
  });

  it("finished agents celebrate, then walk out and disappear", () => {
    const scene = new OfficeScene(DEFAULT_LAYOUT);
    scene.sync([agent("a")], 0);
    scene.sync([agent("a", { ended: true, endedAt: 1000, activity: "DONE" })], 1000);
    expect(scene.entities.get("a")!.zone).not.toBe("exit");
    scene.sync([agent("a", { ended: true, endedAt: 1000, activity: "DONE" })], 1000 + DONE_CELEBRATION_MS + 1);
    expect(scene.entities.get("a")!.zone).toBe("exit");
    settle(scene);
    expect(scene.entities.has("a")).toBe(false);
  });

  it("never spawns agents that already ended", () => {
    const scene = new OfficeScene(DEFAULT_LAYOUT);
    scene.sync([agent("gone", { ended: true, endedAt: 0, activity: "DONE" })], 999_999);
    expect(scene.entities.size).toBe(0);
  });

  it("handles 20 sessions with 50 subagents without running out of spots", () => {
    const scene = new OfficeScene(DEFAULT_LAYOUT);
    const agents: AgentState[] = [];
    for (let i = 0; i < 20; i++) agents.push(agent(`main-${i}`));
    for (let i = 0; i < 50; i++) agents.push(agent(`sub-${i}`, { isMain: false, parentKey: `main-${i % 20}` }));
    scene.sync(agents, 0);
    expect(scene.entities.size).toBe(70);
    const seated = [...scene.entities.values()].filter((e) => e.seatId !== null);
    expect(seated.length).toBe(70);
  });
});
