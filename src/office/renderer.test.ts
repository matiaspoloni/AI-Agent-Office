// @vitest-environment node
import { describe, expect, it } from "vitest";
import type { Activity } from "../bindings/Activity";
import type { AgentState } from "../bindings/AgentState";
import type { ProviderInfo } from "../bindings/ProviderInfo";
import { DEFAULT_LAYOUT } from "./layout";
import { frameFor, OfficeRenderer } from "./renderer";
import { type Entity, OfficeScene, type TeamResolver } from "./scene";

/** A 2D context that only counts calls and remembers fill colors: enough to run the renderer without a browser. */
function fakeContext(calls: { n: number }, fills?: Set<string>): CanvasRenderingContext2D {
  const target: Record<string, unknown> = {
    measureText: (text: string) => ({ width: text.length * 6 }),
  };
  return new Proxy(target, {
    get(obj, prop: string) {
      if (prop in obj) return obj[prop];
      return () => {
        calls.n++;
      };
    },
    set(obj, prop: string, value) {
      obj[prop] = value;
      if (prop === "fillStyle") fills?.add(String(value));
      return true;
    },
  }) as unknown as CanvasRenderingContext2D;
}

const ACTIVITIES: Activity[] = ["IDLE", "THINKING", "READING", "CODING", "RUNNING_COMMAND", "TESTING", "WAITING_PERMISSION", "WAITING_INPUT", "ERROR", "DONE"];

function agent(key: string, i: number, partial: Partial<AgentState> = {}): AgentState {
  return {
    key,
    sessionKey: `demo:${key}`,
    provider: "demo",
    sessionId: key,
    agentId: key,
    isMain: true,
    name: `Agent ${i}`,
    activity: ACTIVITIES[i % ACTIVITIES.length],
    activitySince: 0,
    runningTools: [],
    toolCalls: 0,
    ended: false,
    ...partial,
  };
}

const providers: Record<string, ProviderInfo> = {
  demo: { descriptor: { id: "demo", displayName: "Demo", accentColor: "#f2c94c", badge: "SIM", executableNames: [], simulated: true } } as unknown as ProviderInfo,
};
const byTeam: TeamResolver = (a) => ({ key: `t${a.key.split("-")[1] ?? 0}`, name: "Team" });

describe("OfficeRenderer", () => {
  it("draws 20 leads + 50 subagents in every activity and returns a hitbox per character", () => {
    const agents: AgentState[] = [];
    for (let i = 0; i < 20; i++) agents.push(agent(`lead-${i % 6}-${i}`, i));
    for (let i = 0; i < 50; i++) agents.push(agent(`sub-${i % 6}-${i}`, i, { isMain: false, parentKey: `lead-${i % 6}-${i % 20}` }));
    const scene = new OfficeScene(DEFAULT_LAYOUT);
    scene.sync(agents, 0, byTeam);
    const renderer = new OfficeRenderer(DEFAULT_LAYOUT);
    const calls = { n: 0 };
    const record = Object.fromEntries(agents.map((a) => [a.key, a]));
    const boxes = renderer.draw(fakeContext(calls), 1600, 1000, {
      scene,
      agents: record,
      providers,
      selectedKey: agents[0].key,
      hoverKey: agents[25].key,
      pendingApprovals: 3,
      now: 1_000,
    });
    expect(boxes.filter((b) => b.kind === "agent").length).toBe(scene.entities.size);
    expect(boxes.some((b) => b.kind === "inbox")).toBe(true);
    expect(calls.n).toBeGreaterThan(500); // furniture, screens, bubbles, tags…
  });
});

describe("silent agents", () => {
  it("get an hourglass bubble, even as a subagent out of focus", () => {
    const lead = agent("lead-1-0", 0, { activity: "IDLE", sessionKey: "demo:s" });
    const sub = agent("sub-1-1", 1, { activity: "RUNNING_COMMAND", isMain: false, parentKey: lead.key, sessionKey: "demo:s" });
    const scene = new OfficeScene(DEFAULT_LAYOUT);
    scene.sync([lead, sub], 0, byTeam);
    const renderer = new OfficeRenderer(DEFAULT_LAYOUT);
    const draw = (silentSince?: number) => {
      const fills = new Set<string>();
      renderer.draw(fakeContext({ n: 0 }, fills), 1600, 1000, {
        scene,
        agents: { [lead.key]: lead, [sub.key]: sub },
        providers,
        selectedKey: null,
        hoverKey: null,
        pendingApprovals: 0,
        now: 1_000,
        sessions: { "demo:s": { status: "active", silentSince } as never },
      });
      return fills;
    };
    const SILENT_BUBBLE = "#fff4dc";
    expect(draw(undefined).has(SILENT_BUBBLE)).toBe(false);
    expect(draw(500).has(SILENT_BUBBLE)).toBe(true);
  });
});

describe("frameFor", () => {
  const base: Entity = {
    key: "a",
    x: 0,
    y: 0,
    path: [],
    seatId: "desk-0-0",
    zone: "desk",
    activity: "CODING",
    activitySince: 0,
    isMain: true,
    parentKey: null,
    team: "",
    sitting: true,
    facing: 1,
    walkPhase: 0,
    wantZone: "desk",
    wantSince: 0,
    celebrating: false,
    fadeMs: 0,
    gone: false,
  };
  const frame = (partial: Partial<Entity>, now = 0) => frameFor({ ...base, ...partial }, now);

  it("maps what the agent does to a pose", () => {
    expect(["typeA", "typeB"]).toContain(frame({ activity: "CODING" }));
    expect(new Set([0, 170, 340].map((t) => frame({ activity: "CODING" }, t))).size).toBe(2);
    expect(frame({ activity: "READING" })).toBe("read");
    expect(frame({ activity: "READING", sitting: false })).toBe("readStand");
    expect(frame({ activity: "THINKING" })).toBe("think");
    expect(frame({ activity: "ERROR" })).toBe("worried");
    expect(frame({ activity: "IDLE" })).toBe("coffee");
    expect(["raise", "sit"]).toContain(frame({ activity: "WAITING_PERMISSION" }));
    expect(frame({ celebrating: true })).toBe("celebrate");
    expect(["walkA", "walkB"]).toContain(frame({ path: [{ x: 1, y: 1 }], walkPhase: 200 }));
  });
});
