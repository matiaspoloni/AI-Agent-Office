// Office scene logic (no drawing): decides where every agent should be,
// allocates seats, walks characters along grid paths and handles
// arrival/exit. Pure and deterministic so it can be unit-tested.
//
// Placement rules:
// * each project team gets a row of desks (a pod) with its name plate;
//   subagents sit next to their parent, overflowing into the meeting room;
// * running a command → terminal room, testing → QA lab, idle → lounge;
// * a character only walks to another room once the new activity has lasted
//   ZONE_SETTLE_MS, so quick tool calls do not send it back and forth;
// * finished agents celebrate, walk to the door and fade out.

import type { Activity } from "../bindings/Activity";
import type { AgentState } from "../bindings/AgentState";
import { buildGrid, type DeskPod, findPath, type Grid, type OfficeLayout, type Seat, TILE, type Zone } from "./layout";

export const WALK_SPEED_PX_PER_S = 64;
/** How long a finished agent celebrates before walking out. */
export const DONE_CELEBRATION_MS = 1_800;
/** How long a new activity must last before the character changes room. */
export const ZONE_SETTLE_MS = 1_500;
/** How long a leaving character takes to fade out at the door. */
export const EXIT_FADE_MS = 450;

export type TargetZone = Zone | "exit";

/** The project an agent works for; agents of one team share a desk pod. */
export interface Team {
  key: string;
  name: string;
}

export type TeamResolver = (agent: AgentState) => Team;

const NO_TEAM: Team = { key: "", name: "" };

export interface Entity {
  key: string;
  x: number; // feet position in layout pixels
  y: number;
  path: { x: number; y: number }[]; // remaining pixel waypoints
  seatId: string | null;
  zone: TargetZone;
  activity: Activity;
  /** Since when the current activity runs (from the agent state). */
  activitySince: number;
  isMain: boolean;
  parentKey: string | null;
  team: string;
  sitting: boolean;
  facing: -1 | 1;
  walkPhase: number;
  /** The zone the agent currently asks for, and since when. */
  wantZone: TargetZone;
  wantSince: number;
  /** Finished and still celebrating in place. */
  celebrating: boolean;
  /** Milliseconds spent fading out at the door (0 = not leaving yet). */
  fadeMs: number;
  gone: boolean;
}

/** Where an agent wants to be for its current activity. */
export function zoneFor(agent: Pick<AgentState, "activity" | "ended">): TargetZone {
  if (agent.ended || agent.activity === "DONE") return "exit";
  switch (agent.activity) {
    case "RUNNING_COMMAND":
      return "terminal";
    case "TESTING":
      return "qa";
    case "IDLE":
      return "lounge";
    default:
      return "desk";
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
  private readonly podOfSeat = new Map<string, DeskPod>();
  /** Teams sitting in each pod, in arrival order. */
  private readonly podTeams = new Map<string, string[]>();
  private readonly teamNames = new Map<string, string>();
  private initialized = false;

  constructor(layout: OfficeLayout) {
    this.layout = layout;
    this.grid = buildGrid(layout);
    for (const s of layout.seats) this.seats.set(s.id, s);
    for (const pod of layout.pods) {
      this.podTeams.set(pod.id, []);
      for (const id of pod.seatIds) this.podOfSeat.set(id, pod);
    }
  }

  seat(id: string | null): Seat | undefined {
    return id ? this.seats.get(id) : undefined;
  }

  /** Agent currently occupying a seat (used to light monitors). */
  occupant(seatId: string): Entity | undefined {
    const key = this.seatOwner.get(seatId);
    const entity = key ? this.entities.get(key) : undefined;
    return entity && entity.seatId === seatId && entity.path.length === 0 ? entity : undefined;
  }

  /** Name plates: the teams sitting in each pod. */
  podLabels(): { pod: DeskPod; names: string[] }[] {
    const out: { pod: DeskPod; names: string[] }[] = [];
    for (const pod of this.layout.pods) {
      const names = (this.podTeams.get(pod.id) ?? []).map((t) => this.teamNames.get(t) ?? "").filter(Boolean);
      if (names.length) out.push({ pod, names });
    }
    return out;
  }

  /** The pod a team sits in (assigned on first use). */
  podOf(team: string): DeskPod | undefined {
    if (!team) return undefined;
    for (const pod of this.layout.pods) if (this.podTeams.get(pod.id)?.includes(team)) return pod;
    return undefined;
  }

  private assignPod(team: string): DeskPod | undefined {
    const existing = this.podOf(team);
    if (existing || !team) return existing;
    const freeDesks = (pod: DeskPod) => pod.seatIds.filter((id) => !this.seatOwner.has(id)).length;
    // An empty pod first; when every pod has a team, share the emptiest one.
    const pod =
      this.layout.pods.find((p) => (this.podTeams.get(p.id) ?? []).length === 0) ??
      [...this.layout.pods].sort((a, b) => freeDesks(b) - freeDesks(a))[0];
    if (pod) this.podTeams.get(pod.id)?.push(team);
    return pod;
  }

  private entrancePixel() {
    const { x, y } = this.layout.entrance;
    return { x: x * TILE + TILE / 2, y: y * TILE + TILE - 1 };
  }

  private isFree(seatId: string, agentKey: string) {
    const owner = this.seatOwner.get(seatId);
    return owner === undefined || owner === agentKey;
  }

  /** A free desk: in the team's pod (next to the parent for subagents). */
  private allocateDesk(entity: Entity): Seat | undefined {
    const free = (id: string) => this.isFree(id, entity.key);
    const pod = this.assignPod(entity.team);
    if (pod) {
      const parentDesk = entity.parentKey ? this.seat(this.homeDesk.get(entity.parentKey) ?? null) : undefined;
      const candidates = pod.seatIds.filter(free).map((id) => this.seats.get(id)!);
      if (parentDesk) candidates.sort((a, b) => Math.abs(a.x - parentDesk.x) - Math.abs(b.x - parentDesk.x));
      if (candidates.length) return candidates[0];
    }
    if (!entity.isMain) return undefined; // subagents overflow into the meeting room
    // Main agents: a desk in a pod nobody uses, then any free desk.
    const unused = this.layout.pods.find((p) => (this.podTeams.get(p.id) ?? []).length === 0 && p.seatIds.some(free));
    const id = unused?.seatIds.find(free) ?? this.layout.seats.find((s) => s.zone === "desk" && free(s.id))?.id;
    return id ? this.seats.get(id) : undefined;
  }

  private allocate(entity: Entity, zone: Zone): Seat | undefined {
    if (zone === "desk") {
      const home = this.homeDesk.get(entity.key);
      if (home) return this.seats.get(home);
      const desk = this.allocateDesk(entity);
      if (desk) {
        this.homeDesk.set(entity.key, desk.id);
        return desk;
      }
    }
    for (const candidateZone of FALLBACKS[zone]) {
      if (candidateZone === "desk") continue; // desks are handed out above
      for (const seat of this.layout.seats) {
        if (seat.zone === candidateZone && this.isFree(seat.id, entity.key)) return seat;
      }
    }
    // Office is crowded: any free spot is better than no spot.
    return this.layout.seats.find((seat) => this.isFree(seat.id, entity.key));
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
    entity.wantZone = zone;
    if (zone === "exit") {
      this.release(entity, false);
      const home = this.homeDesk.get(entity.key);
      if (home && this.seatOwner.get(home) === entity.key) this.seatOwner.delete(home);
      this.homeDesk.delete(entity.key);
      this.routeFrom(entity, start, this.entrancePixel(), this.layout.entrance);
      return;
    }
    this.release(entity, true);
    const seat = this.allocate(entity, zone);
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

  /** Frees pods whose team has left the office. */
  private releasePods() {
    const present = new Set<string>();
    for (const e of this.entities.values()) present.add(e.team);
    for (const [podId, teams] of this.podTeams) {
      const kept = teams.filter((t) => present.has(t));
      if (kept.length !== teams.length) this.podTeams.set(podId, kept);
    }
  }

  /**
   * Reconciles entities with the latest agents. New agents walk in from the
   * entrance (except on the very first sync, where everyone is placed
   * directly). `teamOf` groups agents by project; without it every agent is
   * teamless and takes any free desk.
   */
  sync(agents: AgentState[], now: number, teamOf?: TeamResolver) {
    const firstSync = !this.initialized;
    this.initialized = true;
    this.releasePods();
    const seen = new Set<string>();
    // Lead agents first, so subagents find their parent's desk.
    const ordered = agents.some((a) => !a.isMain) ? [...agents].sort((a, b) => Number(b.isMain) - Number(a.isMain)) : agents;

    for (const agent of ordered) {
      seen.add(agent.key);
      let entity = this.entities.get(agent.key);
      const team = teamOf ? teamOf(agent) : NO_TEAM;
      if (team.key) this.teamNames.set(team.key, team.name);
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
          activitySince: agent.activitySince,
          isMain: agent.isMain,
          parentKey: agent.parentKey ?? null,
          team: team.key,
          sitting: false,
          facing: 1,
          walkPhase: 0,
          wantZone: wanted,
          wantSince: now,
          celebrating: false,
          fadeMs: 0,
          gone: false,
        };
        this.entities.set(agent.key, entity);
        this.sendTo(entity, wanted, firstSync);
      } else if (entity.zone === "exit") {
        // Already on the way out.
      } else if (wanted === entity.zone) {
        entity.wantZone = wanted;
      } else {
        if (entity.wantZone !== wanted) {
          entity.wantZone = wanted;
          entity.wantSince = now;
        }
        if (wanted === "exit" || now - entity.wantSince >= ZONE_SETTLE_MS) this.sendTo(entity, wanted, false);
      }
      entity.activity = agent.activity;
      entity.activitySince = agent.activitySince;
      entity.celebrating = celebrating;
      entity.team = team.key || entity.team;
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
          // At the door: fade out, then leave the office.
          e.fadeMs += dtMs;
          moving = true;
          if (e.fadeMs >= EXIT_FADE_MS) {
            e.gone = true;
            this.entities.delete(key);
          }
        }
      }
    }
    return moving;
  }
}
