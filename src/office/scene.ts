// Office scene logic (no drawing): decides where every agent should be,
// allocates seats, walks characters along grid paths and handles
// arrival/exit. Pure and deterministic so it can be unit-tested.

import type { Activity } from "../bindings/Activity";
import type { AgentState } from "../bindings/AgentState";
import { buildGrid, findPath, type Grid, type OfficeLayout, type Seat, TILE, type Zone } from "./layout";

export const WALK_SPEED_PX_PER_S = 64;
/** How long a finished agent celebrates before walking out. */
export const DONE_CELEBRATION_MS = 1_800;

export type TargetZone = Zone | "exit";

export interface Entity {
  key: string;
  x: number; // feet position in layout pixels
  y: number;
  path: { x: number; y: number }[]; // remaining pixel waypoints
  seatId: string | null;
  zone: TargetZone;
  activity: Activity;
  sitting: boolean;
  facing: -1 | 1;
  walkPhase: number;
  gone: boolean;
}

/** Where an agent wants to be for its current activity. */
export function zoneFor(agent: Pick<AgentState, "activity" | "isMain" | "ended">): TargetZone {
  if (agent.ended || agent.activity === "DONE") return "exit";
  switch (agent.activity) {
    case "RUNNING_COMMAND":
      return "terminal";
    case "TESTING":
      return "qa";
    case "IDLE":
      return "lounge";
    default:
      return agent.isMain ? "desk" : "meeting";
  }
}

const FALLBACKS: Record<Zone, Zone[]> = {
  desk: ["desk", "meeting", "standing"],
  meeting: ["meeting", "desk", "standing"],
  qa: ["qa", "terminal", "standing"],
  terminal: ["terminal", "qa", "standing"],
  lounge: ["lounge", "standing"],
  standing: ["standing"],
};

export function seatPixel(seat: Seat): { x: number; y: number } {
  return {
    x: seat.x * TILE + TILE / 2 + (seat.dx ?? 0),
    y: seat.y * TILE + TILE - 1 + (seat.dy ?? 0),
  };
}

function tileOf(px: { x: number; y: number }): { x: number; y: number } {
  return { x: Math.floor(px.x / TILE), y: Math.floor(px.y / TILE) };
}

export class OfficeScene {
  readonly layout: OfficeLayout;
  readonly grid: Grid;
  readonly entities = new Map<string, Entity>();
  private readonly seats = new Map<string, Seat>();
  private readonly seatOwner = new Map<string, string>();
  private readonly homeDesk = new Map<string, string>();
  private initialized = false;

  constructor(layout: OfficeLayout) {
    this.layout = layout;
    this.grid = buildGrid(layout);
    for (const s of layout.seats) this.seats.set(s.id, s);
  }

  seat(id: string | null): Seat | undefined {
    return id ? this.seats.get(id) : undefined;
  }

  /** Agent currently occupying a seat (used to light monitors). */
  occupant(seatId: string): Entity | undefined {
    for (const e of this.entities.values()) if (e.seatId === seatId && e.path.length === 0) return e;
    return undefined;
  }

  private entrancePixel() {
    const { x, y } = this.layout.entrance;
    return { x: x * TILE + TILE / 2, y: y * TILE + TILE - 1 };
  }

  private isFree(seatId: string, agentKey: string) {
    const owner = this.seatOwner.get(seatId);
    return owner === undefined || owner === agentKey;
  }

  private allocate(agentKey: string, zone: Zone): Seat | undefined {
    if (zone === "desk") {
      const home = this.homeDesk.get(agentKey);
      if (home) return this.seats.get(home);
    }
    for (const candidateZone of FALLBACKS[zone]) {
      for (const seat of this.layout.seats) {
        if (seat.zone === candidateZone && this.isFree(seat.id, agentKey)) {
          if (candidateZone === "desk" && zone === "desk") this.homeDesk.set(agentKey, seat.id);
          return seat;
        }
      }
    }
    // Office is crowded: any free spot is better than no spot.
    return this.layout.seats.find((seat) => this.isFree(seat.id, agentKey));
  }

  private release(entity: Entity, keepHome: boolean) {
    if (entity.seatId && !(keepHome && this.homeDesk.get(entity.key) === entity.seatId)) {
      if (this.seatOwner.get(entity.seatId) === entity.key) this.seatOwner.delete(entity.seatId);
    }
    entity.seatId = null;
  }

  /** Tile the character stands on (its seat tile when seated: seated sprites are offset into the desk). */
  private currentTile(entity: Entity): { x: number; y: number } {
    const seat = this.seat(entity.seatId);
    return seat && entity.path.length === 0 ? { x: seat.x, y: seat.y } : tileOf(entity);
  }

  private routeFrom(
    entity: Entity,
    startTile: { x: number; y: number },
    target: { x: number; y: number },
    goalTile: { x: number; y: number },
  ) {
    const tiles = findPath(this.grid, startTile, goalTile);
    const waypoints = tiles.map((t) => ({ x: t.x * TILE + TILE / 2, y: t.y * TILE + TILE - 1 }));
    waypoints.pop(); // replace the last tile center with the exact seat position
    waypoints.push(target);
    entity.path = waypoints;
    entity.sitting = false;
  }

  private sendTo(entity: Entity, zone: TargetZone, instant: boolean) {
    const start = this.currentTile(entity);
    entity.zone = zone;
    if (zone === "exit") {
      this.release(entity, false);
      const home = this.homeDesk.get(entity.key);
      if (home && this.seatOwner.get(home) === entity.key) this.seatOwner.delete(home);
      this.homeDesk.delete(entity.key);
      this.routeFrom(entity, start, this.entrancePixel(), this.layout.entrance);
      return;
    }
    this.release(entity, true);
    const seat = this.allocate(entity.key, zone);
    if (!seat) {
      entity.path = [];
      return;
    }
    this.seatOwner.set(seat.id, entity.key);
    entity.seatId = seat.id;
    const target = seatPixel(seat);
    if (instant) {
      entity.x = target.x;
      entity.y = target.y;
      entity.path = [];
      entity.sitting = seat.sitting;
    } else {
      this.routeFrom(entity, start, target, { x: seat.x, y: seat.y });
    }
  }

  /**
   * Reconciles entities with the latest agents. New agents walk in from the
   * entrance (except on the very first sync, where everyone is placed directly).
   */
  sync(agents: AgentState[], now: number) {
    const firstSync = !this.initialized;
    this.initialized = true;
    const seen = new Set<string>();

    for (const agent of agents) {
      seen.add(agent.key);
      let entity = this.entities.get(agent.key);
      const celebrating = agent.ended && agent.endedAt !== undefined && now - agent.endedAt < DONE_CELEBRATION_MS;
      const wanted: TargetZone = celebrating ? (entity?.zone ?? "exit") : zoneFor(agent);

      if (!entity) {
        if (agent.ended) continue; // never spawn someone who already left
        const start = this.entrancePixel();
        entity = {
          key: agent.key,
          x: start.x,
          y: start.y,
          path: [],
          seatId: null,
          zone: "standing",
          activity: agent.activity,
          sitting: false,
          facing: 1,
          walkPhase: 0,
          gone: false,
        };
        this.entities.set(agent.key, entity);
        this.sendTo(entity, wanted, firstSync);
      } else if (entity.zone !== wanted) {
        this.sendTo(entity, wanted, false);
      }
      entity.activity = agent.activity;
    }

    for (const [key, entity] of this.entities) {
      if (!seen.has(key) && entity.zone !== "exit") this.sendTo(entity, "exit", false);
    }
  }

  /** Advances walking animations. Returns true while anything is moving. */
  step(dtMs: number): boolean {
    let moving = false;
    const budget = (WALK_SPEED_PX_PER_S * dtMs) / 1000;
    for (const [key, e] of this.entities) {
      let remaining = budget;
      while (remaining > 0 && e.path.length) {
        const next = e.path[0];
        const dx = next.x - e.x;
        const dy = next.y - e.y;
        const dist = Math.hypot(dx, dy);
        if (dx !== 0) e.facing = dx > 0 ? 1 : -1;
        if (dist <= remaining) {
          e.x = next.x;
          e.y = next.y;
          e.path.shift();
          remaining -= dist;
        } else {
          e.x += (dx / dist) * remaining;
          e.y += (dy / dist) * remaining;
          remaining = 0;
        }
      }
      if (e.path.length) {
        moving = true;
        e.walkPhase += dtMs;
      } else {
        e.walkPhase = 0;
        e.sitting = !!this.seat(e.seatId)?.sitting;
        if (e.zone === "exit") {
          e.gone = true;
          this.entities.delete(key);
        }
      }
    }
    return moving;
  }
}
