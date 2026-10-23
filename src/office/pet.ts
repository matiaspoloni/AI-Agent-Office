// The office cat. It wanders the floor between naps, and when someone has
// been stuck (waiting for you, or in an error) it goes and curls up next to
// whoever has been stuck the longest: one more hint of where to look.

import type { Activity } from "../bindings/Activity";
import { findPath, type Grid, TILE } from "./layout";
import type { Entity } from "./scene";
import { hashString } from "./sprites";

const SPEED = 30; // layout px per second
const STUCK: ReadonlySet<Activity> = new Set(["WAITING_PERMISSION", "WAITING_INPUT", "ERROR"]);

export type PetFrame = "walkA" | "walkB" | "sit" | "sleep";

export class OfficePet {
  x: number;
  y: number;
  facing: -1 | 1 = 1;
  /** Next to the agent it keeps company, or resting between walks. */
  resting = false;
  /** Whose side it curls up at (null while wandering). */
  companion: string | null = null;
  private path: { x: number; y: number }[] = [];
  private goal = -1;
  private restUntil = 0;
  private walked = 0;
  private wanders = 0;

  constructor(private readonly grid: Grid, start: { x: number; y: number }) {
    this.x = start.x * TILE + TILE / 2;
    this.y = start.y * TILE + TILE - 3;
  }

  private tile() {
    return { x: Math.floor(this.x / TILE), y: Math.floor(this.y / TILE) };
  }

  private walkTo(tx: number, ty: number) {
    const goal = ty * this.grid.width + tx;
    if (goal === this.goal) return;
    this.goal = goal;
    this.resting = false;
    this.path = findPath(this.grid, this.tile(), { x: tx, y: ty }).map((t) => ({ x: t.x * TILE + TILE / 2, y: t.y * TILE + TILE - 3 }));
  }

  /** A walkable tile next to (tx, ty), or null. */
  private beside(tx: number, ty: number) {
    for (const [dx, dy] of [[1, 0], [-1, 0], [0, 1], [0, -1], [1, 1], [-1, 1]]) {
      const x = tx + dx;
      const y = ty + dy;
      if (x >= 0 && y >= 0 && x < this.grid.width && y < this.grid.height && this.grid.walkable[y * this.grid.width + x]) return { x, y };
    }
    return null;
  }

  step(dtMs: number, now: number, entities: Iterable<Entity>) {
    // Whoever has been stuck the longest, if anyone.
    let stuck: Entity | undefined;
    for (const e of entities) if (e.fadeMs === 0 && !e.path.length && STUCK.has(e.activity) && (!stuck || e.activitySince < stuck.activitySince)) stuck = e;
    if (stuck) {
      const spot = this.beside(Math.floor(stuck.x / TILE), Math.floor((stuck.y - 1) / TILE));
      this.companion = stuck.key;
      if (spot) this.walkTo(spot.x, spot.y);
    } else {
      if (this.companion) this.restUntil = 0;
      this.companion = null;
      if (!this.path.length && now >= this.restUntil) {
        // Wander to another floor tile, chosen deterministically.
        const n = hashString(`pet:${this.wanders++}`);
        for (let i = 0; i < 40; i++) {
          const idx = (n + i * 7919) % this.grid.cells.length;
          if (this.grid.walkable[idx] && this.grid.cells[idx] === "floor") {
            this.walkTo(idx % this.grid.width, Math.floor(idx / this.grid.width));
            break;
          }
        }
        this.restUntil = Number.POSITIVE_INFINITY;
      }
    }

    let budget = (SPEED * Math.min(dtMs, 100)) / 1000;
    while (budget > 0 && this.path.length) {
      const next = this.path[0];
      const dx = next.x - this.x;
      const dy = next.y - this.y;
      const dist = Math.hypot(dx, dy);
      if (dx !== 0) this.facing = dx > 0 ? 1 : -1;
      if (dist <= budget) {
        this.x = next.x;
        this.y = next.y;
        this.path.shift();
        budget -= dist;
      } else {
        this.x += (dx / dist) * budget;
        this.y += (dy / dist) * budget;
        budget = 0;
      }
      this.walked += dtMs;
    }
    if (!this.path.length && !this.resting) {
      this.resting = true;
      if (!this.companion) this.restUntil = now + 6000 + (hashString(`rest:${this.wanders}`) % 9000);
    }
  }

  frame(now: number): PetFrame {
    if (!this.resting) return Math.floor(this.walked / 160) % 2 ? "walkA" : "walkB";
    return this.companion || now % 20_000 > 12_000 ? "sleep" : "sit";
  }
}

/** Cat sprites, facing right: t fur, s stripes, w eyes, p nose. */
export const PET_FRAMES: Record<PetFrame, string[]> = {
  walkA: ["........t.t", "........ttt", "t.......twt", ".t.tsstsstp", "..ttttttt..", "..t.t..t.t."],
  walkB: ["........t.t", "........ttt", "t.......twt", ".t.tsstsstp", "..ttttttt..", "...t.t..t.t"],
  sit: ["......t.t..", "......ttt..", "......twtp.", "..tt.ttttt.", ".t.tsstt...", "..tttttt..."],
  sleep: ["...........", "...........", "....tsst...", "..tttttttt.", ".tttssssttt", "..ttttttt.."],
};

export const PET_PALETTE = { t: "#9a8f86", s: "#6e655e", w: "#e8d44a", p: "#d98a8a" };
