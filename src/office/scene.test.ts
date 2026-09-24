import { describe, expect, it } from "vitest";
import type { AgentState } from "../bindings/AgentState";
import { buildGrid, DEFAULT_LAYOUT, findPath, type OfficeLayout, validateLayout } from "./layout";
import { DONE_CELEBRATION_MS, EXIT_FADE_MS, OfficeScene, type TeamResolver, ZONE_SETTLE_MS, zoneFor } from "./scene";

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

/** Team = the part of the key before the first "-" (e.g. "nalu-sub-1" → nalu). */
const byPrefix: TeamResolver = (a) => {
  const team = a.key.split("-")[0];
  return { key: team, name: team.toUpperCase() };
};

describe("layout", () => {
  it("the default layout is valid", () => {
    expect(validateLayout(DEFAULT_LAYOUT)).toEqual([]);
  });

  it("validation reports broken layouts", () => {
    const broken: OfficeLayout = structuredClone(DEFAULT_LAYOUT);
    broken.furniture.push({ ...broken.furniture[0] }); // same id, same place
    broken.seats.push({ id: "void-seat", zone: "lounge", roomId: "lounge", x: 0, y: 0, sitting: false });
    broken.pods[0].seatIds.push("nope");
    const problems = validateLayout(broken).join("\n");
    expect(problems).toContain("duplicate furniture id");
    expect(problems).toContain("overlaps");
    expect(problems).toContain("void-seat cannot be reached");
    expect(problems).toContain("unknown seat nope");
  });

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
    expect(zoneFor({ activity: "CODING", ended: false })).toBe("desk");
    expect(zoneFor({ activity: "READING", ended: false })).toBe("desk");
    expect(zoneFor({ activity: "THINKING", ended: false })).toBe("desk");
    expect(zoneFor({ activity: "RUNNING_COMMAND", ended: false })).toBe("terminal");
    expect(zoneFor({ activity: "TESTING", ended: false })).toBe("qa");
    expect(zoneFor({ activity: "IDLE", ended: false })).toBe("lounge");
    expect(zoneFor({ activity: "WAITING_PERMISSION", ended: false })).toBe("desk");
    expect(zoneFor({ activity: "CODING", ended: true })).toBe("exit");
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
    scene.sync([agent("a", { activity: "IDLE" })], 1 + ZONE_SETTLE_MS);
    settle(scene);
    expect(scene.seat(entity.seatId)?.zone).toBe("lounge");

    // Someone else cannot take the reserved desk meanwhile.
    const t = 2 + ZONE_SETTLE_MS;
    scene.sync([agent("a", { activity: "IDLE" }), agent("b")], t);
    expect(scene.entities.get("b")!.seatId).not.toBe(desk);

    scene.sync([agent("a"), agent("b")], t + 1);
    scene.sync([agent("a"), agent("b")], t + 1 + ZONE_SETTLE_MS);
    settle(scene);
    expect(entity.seatId).toBe(desk);
  });

  it("waits until a new activity lasts before changing room", () => {
    const scene = new OfficeScene(DEFAULT_LAYOUT);
    scene.sync([agent("a")], 0);
    const entity = scene.entities.get("a")!;
    const desk = entity.seatId;
    // A quick command: back to coding before it settles → nobody walks.
    scene.sync([agent("a", { activity: "RUNNING_COMMAND" })], 100);
    scene.sync([agent("a", { activity: "RUNNING_COMMAND" })], 100 + ZONE_SETTLE_MS - 1);
    scene.sync([agent("a")], 100 + ZONE_SETTLE_MS);
    expect(entity.seatId).toBe(desk);
    expect(entity.path.length).toBe(0);
    // A long one: the character goes to the terminal room.
    scene.sync([agent("a", { activity: "RUNNING_COMMAND" })], 5_000);
    expect(entity.zone).toBe("desk");
    scene.sync([agent("a", { activity: "RUNNING_COMMAND" })], 5_000 + ZONE_SETTLE_MS);
    expect(entity.zone).toBe("terminal");
    expect(entity.path.length).toBeGreaterThan(0);
  });

  it("gives each project a desk row and seats subagents next to their lead", () => {
    const scene = new OfficeScene(DEFAULT_LAYOUT);
    scene.sync(
      [
        agent("nalu-sub-1", { isMain: false, parentKey: "nalu" }),
        agent("nalu"),
        agent("atlas"),
        agent("nalu-sub-2", { isMain: false, parentKey: "nalu" }),
      ],
      0,
      byPrefix,
    );
    const pod = (key: string) => DEFAULT_LAYOUT.pods.find((p) => p.seatIds.includes(scene.entities.get(key)!.seatId!))?.id;
    expect(pod("nalu")).toBeDefined();
    expect(pod("atlas")).toBeDefined();
    expect(pod("nalu")).not.toBe(pod("atlas"));
    expect(pod("nalu-sub-1")).toBe(pod("nalu"));
    expect(pod("nalu-sub-2")).toBe(pod("nalu"));
    const x = (key: string) => scene.seat(scene.entities.get(key)!.seatId)!.x;
    expect(Math.abs(x("nalu-sub-1") - x("nalu"))).toBe(4); // the neighbouring desk
    expect(scene.podLabels().map((l) => l.names)).toEqual([["NALU"], ["ATLAS"]]);
  });

  it("frees a team's row when the team has left", () => {
    const scene = new OfficeScene(DEFAULT_LAYOUT);
    scene.sync([agent("nalu"), agent("atlas")], 0, byPrefix);
    scene.sync([agent("atlas")], 1, byPrefix);
    settle(scene);
    expect(scene.entities.has("nalu")).toBe(false);
    scene.sync([agent("atlas"), agent("zen")], 2, byPrefix);
    expect(scene.podLabels().map((l) => l.names)).toEqual([["ZEN"], ["ATLAS"]]);
  });

  it("subagents overflow into the meeting room when their row is full", () => {
    const scene = new OfficeScene(DEFAULT_LAYOUT);
    const subs = Array.from({ length: 7 }, (_, i) => agent(`nalu-sub-${i}`, { isMain: false, parentKey: "nalu" }));
    scene.sync([agent("nalu"), ...subs], 0, byPrefix);
    const zones = subs.map((s) => scene.seat(scene.entities.get(s.key)!.seatId)?.zone);
    expect(zones.filter((z) => z === "desk").length).toBe(5);
    expect(zones.filter((z) => z === "meeting").length).toBe(2);
  });

  it("finished agents celebrate, then walk out and disappear", () => {
    const scene = new OfficeScene(DEFAULT_LAYOUT);
    scene.sync([agent("a")], 0);
    scene.sync([agent("a", { ended: true, endedAt: 1000, activity: "DONE" })], 1000);
    expect(scene.entities.get("a")!.zone).not.toBe("exit");
    scene.sync([agent("a", { ended: true, endedAt: 1000, activity: "DONE" })], 1000 + DONE_CELEBRATION_MS + 1);
    expect(scene.entities.get("a")!.zone).toBe("exit");
    for (let i = 0; i < 2000 && scene.entities.get("a")!.path.length; i++) scene.step(100);
    // At the door the character fades out before leaving.
    const leaving = scene.entities.get("a")!;
    expect(leaving.fadeMs).toBeGreaterThan(0);
    scene.step(EXIT_FADE_MS);
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
    for (let i = 0; i < 20; i++) agents.push(agent(`p${i % 6}-main-${i}`));
    for (let i = 0; i < 50; i++) agents.push(agent(`p${i % 6}-sub-${i}`, { isMain: false, parentKey: `p${i % 6}-main-${i % 20}` }));
    scene.sync(agents, 0, byPrefix);
    expect(scene.entities.size).toBe(70);
    const seated = [...scene.entities.values()].filter((e) => e.seatId !== null);
    expect(seated.length).toBe(70);
  });
});

describe("OfficeScene performance", () => {
  it("syncs and animates 70 agents for 10 simulated seconds quickly", () => {
    const scene = new OfficeScene(DEFAULT_LAYOUT);
    const activities = ["CODING", "READING", "RUNNING_COMMAND", "TESTING", "IDLE", "THINKING"] as const;
    let agents: AgentState[] = [];
    for (let i = 0; i < 20; i++) agents.push(agent(`p${i % 6}-main-${i}`));
    for (let i = 0; i < 50; i++) agents.push(agent(`p${i % 6}-sub-${i}`, { isMain: false, parentKey: `p${i % 6}-main-${i % 20}` }));
    const started = performance.now();
    for (let frame = 0; frame < 600; frame++) {
      if (frame % 24 === 0) agents = agents.map((a, i) => ({ ...a, activity: activities[(i + frame) % activities.length] }));
      scene.sync(agents, frame * 16.7, byPrefix);
      scene.step(16.7);
    }
    const perFrame = (performance.now() - started) / 600;
    expect(scene.entities.size).toBe(70);
    // Budget for a whole frame is 16.7 ms; the scene logic must be a small part of it.
    expect(perFrame).toBeLessThan(4);
  });
});
