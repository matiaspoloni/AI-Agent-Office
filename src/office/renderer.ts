// Canvas 2D renderer for the office. Static floors/walls are cached in an
// offscreen layer; furniture and characters are y-sorted every frame so
// seated characters are correctly occluded by their desks. Character frames
// are cached per look (see sprites.ts), so a frame costs a few drawImage
// calls per character.

import type { Activity } from "../bindings/Activity";
import type { AgentState } from "../bindings/AgentState";
import type { ProviderInfo } from "../bindings/ProviderInfo";
import type { SessionState } from "../bindings/SessionState";
import { silentSince } from "../state/silence";
import { type Furniture, type OfficeLayout, TILE } from "./layout";
import { EXIT_FADE_MS, type Entity, type OfficeScene } from "./scene";
import {
  type CharacterFrame,
  type CharacterLook,
  characterCanvas,
  characterLook,
  hashString,
  ICON_PALETTE,
  ICONS,
  SPRITE_H,
  SPRITE_W,
  spriteCanvas,
} from "./sprites";

export interface Hitbox {
  key: string;
  kind: "agent" | "inbox";
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface RenderInput {
  scene: OfficeScene;
  agents: Record<string, AgentState>;
  providers: Record<string, ProviderInfo>;
  selectedKey: string | null;
  hoverKey: string | null;
  pendingApprovals: number;
  now: number;
  /** Sessions, to show which busy agents have gone quiet (optional). */
  sessions?: Record<string, SessionState>;
}

const ATTENTION: ReadonlySet<Activity> = new Set(["WAITING_PERMISSION", "WAITING_INPUT", "ERROR"]);

const FLOORS: Record<string, (ctx: CanvasRenderingContext2D, x: number, y: number) => void> = {
  wood(ctx, x, y) {
    ctx.fillStyle = "#8a6a4a";
    ctx.fillRect(x, y, TILE, TILE);
    ctx.fillStyle = "#7a5c3f";
    for (let i = 0; i < 4; i++) ctx.fillRect(x, y + i * 4 + 3, TILE, 1);
    ctx.fillRect(x + ((y / TILE) % 2 ? 5 : 11), y, 1, TILE);
  },
  carpetBlue(ctx, x, y) {
    ctx.fillStyle = "#3b4a6e";
    ctx.fillRect(x, y, TILE, TILE);
    ctx.fillStyle = "#435481";
    ctx.fillRect(x + 3, y + 3, 2, 2);
    ctx.fillRect(x + 11, y + 11, 2, 2);
  },
  checker(ctx, x, y) {
    for (let i = 0; i < 2; i++)
      for (let j = 0; j < 2; j++) {
        ctx.fillStyle = (i + j) % 2 ? "#cfd4de" : "#e3e7ee";
        ctx.fillRect(x + i * 8, y + j * 8, 8, 8);
      }
  },
  carpet(ctx, x, y) {
    ctx.fillStyle = (x / TILE + y / TILE) % 2 ? "#4a5169" : "#4d546d";
    ctx.fillRect(x, y, TILE, TILE);
    ctx.fillStyle = "#555d78";
    ctx.fillRect(x + 7, y + 7, 1, 1);
  },
  metal(ctx, x, y) {
    ctx.fillStyle = "#353a46";
    ctx.fillRect(x, y, TILE, TILE);
    ctx.fillStyle = "#2c303a";
    ctx.fillRect(x, y + TILE - 1, TILE, 1);
    ctx.fillRect(x + TILE - 1, y, 1, TILE);
    ctx.fillStyle = "#4a5060";
    ctx.fillRect(x + 2, y + 2, 1, 1);
    ctx.fillRect(x + 13, y + 13, 1, 1);
  },
  warm(ctx, x, y) {
    ctx.fillStyle = "#7c5b4a";
    ctx.fillRect(x, y, TILE, TILE);
    ctx.fillStyle = "#86644f";
    ctx.fillRect(x + 1, y + 1, 6, 6);
    ctx.fillRect(x + 9, y + 9, 6, 6);
  },
  corridor(ctx, x, y) {
    ctx.fillStyle = "#b9b2a4";
    ctx.fillRect(x, y, TILE, TILE);
    ctx.fillStyle = "#aaa395";
    ctx.fillRect(x, y + TILE - 1, TILE, 1);
    ctx.fillRect(x + TILE - 1, y, 1, TILE);
  },
};

function drawStatic(layout: OfficeLayout, scene: OfficeScene): HTMLCanvasElement | null {
  if (typeof document === "undefined") return null;
  const canvas = document.createElement("canvas");
  canvas.width = layout.width * TILE;
  canvas.height = layout.height * TILE;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  ctx.fillStyle = "#151821";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  const { grid } = scene;
  const floorOf = new Map(layout.rooms.map((r) => [r.id, r.floor]));
  for (let ty = 0; ty < grid.height; ty++) {
    for (let tx = 0; tx < grid.width; tx++) {
      const i = ty * grid.width + tx;
      const cell = grid.cells[i];
      const x = tx * TILE;
      const y = ty * TILE;
      if (cell === "corridor") FLOORS.corridor(ctx, x, y);
      else if (cell === "floor" || cell === "door") {
        const floor = floorOf.get(grid.roomAt[i] ?? "") ?? "carpet";
        (FLOORS[floor] ?? FLOORS.carpet)(ctx, x, y);
        if (cell === "door") {
          ctx.fillStyle = "rgba(0,0,0,0.12)";
          ctx.fillRect(x, y, TILE, TILE);
        }
      } else if (cell === "wall") {
        ctx.fillStyle = "#3a4052";
        ctx.fillRect(x, y, TILE, TILE);
        ctx.fillStyle = "#566078";
        ctx.fillRect(x, y, TILE, 4);
        ctx.fillStyle = "#2c3140";
        ctx.fillRect(x, y + TILE - 2, TILE, 2);
      }
    }
  }
  for (const d of layout.decorations) {
    const x = d.x * TILE;
    const y = d.y * TILE;
    if (d.kind === "clock") {
      ctx.fillStyle = "#e9e4d8";
      ctx.fillRect(x + 4, y + 4, 8, 8);
      ctx.fillStyle = "#2b2f3a";
      ctx.fillRect(x + 7, y + 5, 1, 4);
      ctx.fillRect(x + 8, y + 8, 3, 1);
    } else if (d.kind === "poster") {
      ctx.fillStyle = "#f2c94c";
      ctx.fillRect(x + 2, y + 3, 12, 9);
      ctx.fillStyle = "#d97757";
      ctx.fillRect(x + 4, y + 5, 8, 2);
      ctx.fillStyle = "#3bd1c6";
      ctx.fillRect(x + 4, y + 8, 5, 2);
    }
  }
  // Entrance mat.
  ctx.fillStyle = "#6b4a3a";
  ctx.fillRect(layout.entrance.x * TILE, layout.entrance.y * TILE + 2, TILE, TILE - 4);
  return canvas;
}

/** What a screen shows for the activity of the person using it. */
function drawScreen(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, activity: Activity | null, now: number, seed: number) {
  if (!activity || activity === "IDLE" || activity === "DONE") {
    ctx.fillStyle = "#30343f";
    ctx.fillRect(x, y, w, h);
    return;
  }
  const scroll = Math.floor(now / 350 + seed);
  switch (activity) {
    case "CODING":
    case "THINKING": {
      ctx.fillStyle = "#1b2130";
      ctx.fillRect(x, y, w, h);
      const colors = ["#3bd1c6", "#b58cff", "#f2c94c", "#6aa8ff"];
      for (let row = 0; row < h - 1; row += 2) {
        const n = hashString(`${seed}:${scroll + row}`);
        if (activity === "THINKING" && row > 1) break;
        ctx.fillStyle = colors[n % colors.length];
        ctx.fillRect(x + 1 + (n % 2), y + 1 + row, 1 + ((n >>> 4) % (w - 2)), 1);
      }
      if (activity === "THINKING" && Math.floor(now / 400) % 2) {
        ctx.fillStyle = "#e7e9ef";
        ctx.fillRect(x + 1, y + 3, 1, 2);
      }
      return;
    }
    case "READING": {
      ctx.fillStyle = "#eef1f6";
      ctx.fillRect(x, y, w, h);
      ctx.fillStyle = "#9aa3b5";
      for (let row = 1; row < h - 1; row += 2) {
        const n = hashString(`${seed}:${scroll + row}`);
        ctx.fillRect(x + 1, y + row, 2 + (n % (w - 3)), 1);
      }
      return;
    }
    case "RUNNING_COMMAND":
    case "TESTING": {
      ctx.fillStyle = "#0c1410";
      ctx.fillRect(x, y, w, h);
      ctx.fillStyle = activity === "TESTING" ? "#7bdc6b" : "#3fdc7a";
      for (let row = 0; row < h - 1; row += 2) {
        const n = hashString(`${seed}:${scroll + row}`);
        ctx.fillRect(x + 1, y + 1 + row, 1 + (n % (w - 2)), 1);
      }
      return;
    }
    case "WAITING_PERMISSION":
    case "WAITING_INPUT": {
      ctx.fillStyle = "#1b2130";
      ctx.fillRect(x, y, w, h);
      const blink = Math.floor(now / 350) % 2 === 0;
      ctx.fillStyle = activity === "WAITING_PERMISSION" ? (blink ? "#ff5c5c" : "#a83a3a") : blink ? "#f2994a" : "#9c5d2a";
      ctx.fillRect(x + 1, y + 2, w - 2, h - 4);
      ctx.fillStyle = "#ffffff";
      ctx.fillRect(x + Math.floor(w / 2), y + 3, 1, Math.max(1, h - 7));
      ctx.fillRect(x + Math.floor(w / 2), y + h - 3, 1, 1);
      return;
    }
    case "ERROR": {
      ctx.fillStyle = Math.floor(now / 200) % 2 ? "#7a1f1f" : "#3a1414";
      ctx.fillRect(x, y, w, h);
      ctx.fillStyle = "#ff8a8a";
      ctx.fillRect(x + 1, y + 1, w - 2, 1);
      return;
    }
  }
}

function drawFurniture(ctx: CanvasRenderingContext2D, f: Furniture, now: number, activity: Activity | null) {
  const x = f.x * TILE;
  const y = f.y * TILE;
  const w = f.w * TILE;
  const seed = f.x * 31 + f.y * 7;
  switch (f.kind) {
    case "desk":
    case "console":
    case "testBench": {
      const top = f.kind === "testBench" ? "#e6e9ef" : f.kind === "console" ? "#5a6072" : "#a0703f";
      const edge = f.kind === "testBench" ? "#b8bfcc" : f.kind === "console" ? "#434858" : "#7a5230";
      ctx.fillStyle = edge;
      ctx.fillRect(x + 1, y + 11, 2, 5);
      ctx.fillRect(x + w - 3, y + 11, 2, 5);
      ctx.fillStyle = top;
      ctx.fillRect(x, y + 1, w, 8);
      ctx.fillStyle = edge;
      ctx.fillRect(x, y + 9, w, 3);
      if (f.kind === "testBench") {
        // Test rig: a small screen with a progress bar and a status light.
        ctx.fillStyle = "#2b2f3a";
        ctx.fillRect(x + 3, y - 4, 10, 7);
        if (activity) {
          const failing = activity === "ERROR";
          const progress = failing ? 6 : 1 + (Math.floor(now / 250 + seed) % 7);
          ctx.fillStyle = failing ? "#ff5c5c" : "#7bdc6b";
          ctx.fillRect(x + 4, y - 1, progress, 2);
          ctx.fillStyle = Math.floor(now / 500) % 2 ? (failing ? "#ff5c5c" : "#3fbf5f") : "#2f5f3a";
        } else {
          ctx.fillStyle = "#2f5f3a";
        }
        ctx.fillRect(x + 4, y - 3, 2, 1);
        ctx.fillStyle = "#9aa3b5";
        ctx.fillRect(x + 22, y + 2, 6, 3);
        break;
      }
      // Monitor (to the side of the person) and keyboard.
      ctx.fillStyle = "#23262f";
      ctx.fillRect(x + 1, y - 7, 9, 9);
      ctx.fillStyle = "#3a3f4d";
      ctx.fillRect(x + 4, y + 2, 3, 2);
      drawScreen(ctx, x + 2, y - 6, 7, 7, f.kind === "console" && activity ? "RUNNING_COMMAND" : activity, now, seed);
      ctx.fillStyle = "#c9ced8";
      ctx.fillRect(x + 12, y + 3, 10, 3);
      ctx.fillStyle = "#9aa1ae";
      ctx.fillRect(x + 12, y + 5, 10, 1);
      break;
    }
    case "ceoDesk": {
      ctx.fillStyle = "#5b3a24";
      ctx.fillRect(x, y + 1, w, 9);
      ctx.fillStyle = "#43291a";
      ctx.fillRect(x, y + 10, w, 5);
      ctx.fillStyle = "#23262f";
      ctx.fillRect(x + 30, y - 6, 12, 8);
      ctx.fillStyle = "#6aa8ff";
      ctx.fillRect(x + 31, y - 5, 10, 6);
      ctx.fillStyle = "#c9a227";
      ctx.fillRect(x + 18, y + 3, 10, 3);
      break;
    }
    case "meetingTable": {
      ctx.fillStyle = "#6b4a33";
      ctx.fillRect(x + 2, y + 2, w - 4, f.h * TILE - 4);
      ctx.fillStyle = "#86603f";
      ctx.fillRect(x + 4, y + 4, w - 8, f.h * TILE - 10);
      ctx.fillStyle = "#e9e4d8";
      ctx.fillRect(x + 20, y + 10, 8, 6);
      ctx.fillRect(x + 90, y + 12, 8, 6);
      break;
    }
    case "serverRack": {
      ctx.fillStyle = "#1f2230";
      ctx.fillRect(x + 1, y - 10, 14, 26);
      ctx.fillStyle = "#2c3142";
      for (let i = 0; i < 5; i++) ctx.fillRect(x + 3, y - 8 + i * 5, 10, 3);
      for (let i = 0; i < 5; i++) {
        const on = (Math.floor(now / 300) + i + f.x) % 3 !== 0;
        ctx.fillStyle = on ? (i % 2 ? "#3fdc7a" : "#f2c94c") : "#3a3f4d";
        ctx.fillRect(x + 11, y - 7 + i * 5, 1, 1);
      }
      break;
    }
    case "sofa": {
      ctx.fillStyle = "#8f3b4c";
      ctx.fillRect(x, y - 2, w, 8);
      ctx.fillStyle = "#b04a5e";
      ctx.fillRect(x, y + 5, w, 8);
      ctx.fillStyle = "#7a3140";
      ctx.fillRect(x, y - 2, 3, 15);
      ctx.fillRect(x + w - 3, y - 2, 3, 15);
      break;
    }
    case "coffee": {
      ctx.fillStyle = "#9aa1ae";
      ctx.fillRect(x + 2, y - 6, 12, 20);
      ctx.fillStyle = "#2b2f3a";
      ctx.fillRect(x + 4, y - 3, 8, 6);
      ctx.fillStyle = "#ff5c5c";
      ctx.fillRect(x + 11, y + 6, 1, 1);
      ctx.fillStyle = "#f2efe8";
      ctx.fillRect(x + 6, y + 8, 4, 4);
      break;
    }
    case "plant": {
      ctx.fillStyle = "#8b5a3c";
      ctx.fillRect(x + 4, y + 8, 8, 7);
      ctx.fillStyle = "#2f8f4a";
      ctx.fillRect(x + 3, y + 1, 10, 8);
      ctx.fillStyle = "#3fbf5f";
      ctx.fillRect(x + 5, y - 2, 6, 6);
      break;
    }
    case "bookshelf": {
      ctx.fillStyle = "#5b3a24";
      ctx.fillRect(x, y - 10, w, 25);
      const colors = ["#d97757", "#4c8df5", "#3fbf5f", "#f2c94c", "#b58cff"];
      for (let row = 0; row < 3; row++)
        for (let i = 0; i < 7; i++) {
          ctx.fillStyle = colors[(i + row) % colors.length];
          ctx.fillRect(x + 2 + i * 4, y - 8 + row * 8, 3, 6);
        }
      break;
    }
    case "whiteboard": {
      ctx.fillStyle = "#c9ced8";
      ctx.fillRect(x, y - 12, w, 12);
      ctx.fillStyle = "#f7f8fa";
      ctx.fillRect(x + 1, y - 11, w - 2, 10);
      ctx.fillStyle = "#4c8df5";
      ctx.fillRect(x + 4, y - 9, w / 3, 1);
      ctx.fillStyle = "#d97757";
      ctx.fillRect(x + 6, y - 6, w / 2, 1);
      ctx.fillStyle = "#3fbf5f";
      ctx.fillRect(x + w - 14, y - 9, 8, 5);
      break;
    }
    case "waterCooler": {
      ctx.fillStyle = "#e3e7ee";
      ctx.fillRect(x + 4, y + 2, 8, 13);
      ctx.fillStyle = "#6aa8ff";
      ctx.fillRect(x + 5, y - 6, 6, 8);
      break;
    }
  }
}

/** The pose for what the character is doing (sprites.ts has the frames). */
export function frameFor(entity: Entity, now: number): CharacterFrame {
  if (entity.path.length) return Math.floor(entity.walkPhase / 140) % 2 ? "walkA" : "walkB";
  if (entity.celebrating) return "celebrate";
  const sit = entity.sitting;
  const offset = hashString(entity.key) % 7;
  switch (entity.activity) {
    case "CODING":
      return sit ? (Math.floor(now / 170 + offset) % 2 ? "typeA" : "typeB") : "stand";
    case "RUNNING_COMMAND":
    case "TESTING":
      return sit ? (Math.floor(now / 420 + offset) % 3 === 0 ? "typeA" : "sit") : "stand";
    case "READING":
      return sit ? "read" : "readStand";
    case "THINKING":
      return sit ? "think" : "thinkStand";
    case "WAITING_PERMISSION":
    case "WAITING_INPUT":
      // Waves now and then to be noticed.
      return Math.floor(now / 600 + offset) % 4 === 3 ? (sit ? "sit" : "stand") : sit ? "raise" : "raiseStand";
    case "ERROR":
      return sit ? "worried" : "worriedStand";
    case "IDLE":
      return sit ? "coffee" : "coffeeStand";
    case "DONE":
      return "celebrate";
  }
}

function drawIcon(ctx: CanvasRenderingContext2D, name: string, x: number, y: number) {
  const sprite = spriteCanvas(ICONS[name], ICON_PALETTE, `icon:${name}`);
  if (sprite) ctx.drawImage(sprite, x, y);
}

const BUBBLE_ICON: Partial<Record<Activity, string>> = {
  READING: "doc",
  CODING: "code",
  RUNNING_COMMAND: "terminal",
  TESTING: "flask",
  WAITING_PERMISSION: "alert",
  WAITING_INPUT: "question",
  ERROR: "error",
  DONE: "check",
  IDLE: "coffee",
};

function drawThoughtCloud(ctx: CanvasRenderingContext2D, x: number, y: number, now: number) {
  ctx.fillStyle = "#ffffff";
  ctx.strokeStyle = "#2b2f3a";
  ctx.lineWidth = 1;
  // Trail of small puffs from the head to the cloud.
  ctx.fillRect(x - 2, y + 13, 2, 2);
  ctx.fillRect(x, y + 10, 3, 2);
  // Cloud: three overlapping blobs.
  const blobs: [number, number, number, number][] = [
    [x + 1, y + 1, 13, 8],
    [x + 3, y - 1, 9, 3],
    [x, y + 3, 15, 4],
  ];
  ctx.fillStyle = "#2b2f3a";
  for (const [bx, by, bw, bh] of blobs) ctx.fillRect(bx - 1, by - 1, bw + 2, bh + 2);
  ctx.fillStyle = "#ffffff";
  for (const [bx, by, bw, bh] of blobs) ctx.fillRect(bx, by, bw, bh);
  const phase = Math.floor(now / 300) % 4;
  ctx.fillStyle = "#6b5bd6";
  for (let i = 0; i < 3; i++) if (i < phase) ctx.fillRect(x + 3 + i * 3, y + 4, 2, 2);
}

/**
 * Draws the activity bubble; returns false when the activity has none. A busy
 * agent that has gone quiet shows an hourglass instead (only a warning).
 */
function drawBubble(ctx: CanvasRenderingContext2D, entity: Entity, lift: number, now: number, silent = false): boolean {
  const activity = entity.celebrating ? "DONE" : entity.activity;
  const bx = Math.round(entity.x + 3);
  if (activity === "THINKING" && !silent) {
    drawThoughtCloud(ctx, bx, Math.round(entity.y - 32 - lift), now);
    return true;
  }
  const name = silent ? "hourglass" : BUBBLE_ICON[activity];
  if (!name || (activity === "IDLE" && entity.zone !== "lounge")) return false;
  const waiting = activity === "WAITING_PERMISSION" || activity === "WAITING_INPUT";
  const bob = waiting ? Math.round(Math.abs(Math.sin(now / 160)) * -3) : 0;
  const by = Math.round(entity.y - 30 + bob - lift);
  ctx.fillStyle = silent
    ? "#fff4dc"
    : activity === "WAITING_PERMISSION" || activity === "ERROR"
      ? "#fff1f1"
      : "#ffffff";
  ctx.fillRect(bx, by, 11, 10);
  ctx.fillStyle = "#2b2f3a";
  ctx.fillRect(bx, by - 1, 11, 1);
  ctx.fillRect(bx, by + 10, 11, 1);
  ctx.fillRect(bx - 1, by, 1, 10);
  ctx.fillRect(bx + 11, by, 1, 10);
  ctx.fillRect(bx + 1, by + 11, 2, 1);
  ctx.fillRect(bx, by + 12, 1, 1);
  drawIcon(ctx, name, bx + 2, by + 2);
  return true;
}

const CONFETTI = ["#f2c94c", "#ff5c5c", "#3bd1c6", "#b58cff", "#7bdc6b", "#6aa8ff"];

/** Confetti falling around a character that just finished (t in ms since the end). */
function drawConfetti(ctx: CanvasRenderingContext2D, x: number, y: number, t: number, seed: number) {
  for (let i = 0; i < 14; i++) {
    const n = hashString(`${seed}:${i}`);
    const life = ((t + (n % 600)) % 1200) / 1200;
    const px = x + ((n % 29) - 14) + Math.sin(life * 6 + i) * 2;
    const py = y - 34 + life * 30;
    ctx.globalAlpha = 1 - life * 0.6;
    ctx.fillStyle = CONFETTI[i % CONFETTI.length];
    ctx.fillRect(Math.round(px), Math.round(py), i % 3 === 0 ? 2 : 1, 1);
  }
  ctx.globalAlpha = 1;
}

/** A puff of dust where a character leaves the office. */
function drawPoof(ctx: CanvasRenderingContext2D, x: number, y: number, progress: number) {
  const r = 3 + progress * 6;
  ctx.globalAlpha = 0.5 * (1 - progress);
  ctx.fillStyle = "#e7e3da";
  for (let i = 0; i < 6; i++) {
    const a = (i / 6) * Math.PI * 2;
    ctx.fillRect(Math.round(x + Math.cos(a) * r) - 1, Math.round(y - 6 + Math.sin(a) * r * 0.6) - 1, 3, 3);
  }
  ctx.globalAlpha = 1;
}

type Drawable = { y: number; furniture?: Furniture; entity?: Entity };

export class OfficeRenderer {
  private staticLayer: HTMLCanvasElement | null = null;
  private readonly looks = new Map<string, CharacterLook>();
  private readonly textWidths = new Map<string, number>();
  private readonly drawables: Drawable[] = [];
  scale = 2;
  offsetX = 0;
  offsetY = 0;

  constructor(private readonly layout: OfficeLayout) {}

  private fit(width: number, height: number) {
    const lw = this.layout.width * TILE;
    const lh = this.layout.height * TILE;
    const raw = Math.min(width / lw, height / lh);
    this.scale = Math.max(1, Math.floor(raw * 2) / 2);
    if (raw < 1) this.scale = raw;
    this.offsetX = Math.floor((width - lw * this.scale) / 2);
    this.offsetY = Math.floor((height - lh * this.scale) / 2);
  }

  toLayout(px: number, py: number) {
    return { x: (px - this.offsetX) / this.scale, y: (py - this.offsetY) / this.scale };
  }

  /** Screen position (device pixels) of a layout point. */
  toScreen(x: number, y: number) {
    return { x: this.offsetX + x * this.scale, y: this.offsetY + y * this.scale };
  }

  private look(entity: Entity, agent: AgentState | undefined, providers: Record<string, ProviderInfo>): CharacterLook {
    const accent = (agent && providers[agent.provider]?.descriptor.accentColor) || "#9aa3b5";
    const cacheKey = `${entity.key}|${accent}|${entity.isMain}`;
    let look = this.looks.get(cacheKey);
    if (!look) {
      look = characterLook(entity.key, accent, !entity.isMain);
      if (this.looks.size > 2_000) this.looks.clear();
      this.looks.set(cacheKey, look);
    }
    return look;
  }

  private measure(ctx: CanvasRenderingContext2D, text: string): number {
    const key = `${ctx.font}|${text}`;
    let w = this.textWidths.get(key);
    if (w === undefined) {
      w = ctx.measureText(text).width;
      if (this.textWidths.size > 4_000) this.textWidths.clear();
      this.textWidths.set(key, w);
    }
    return w;
  }

  draw(ctx: CanvasRenderingContext2D, width: number, height: number, input: RenderInput): Hitbox[] {
    const { scene, agents, providers, now } = input;
    this.fit(width, height);
    if (!this.staticLayer) this.staticLayer = drawStatic(this.layout, scene);

    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.fillStyle = "#0f1117";
    ctx.fillRect(0, 0, width, height);
    ctx.imageSmoothingEnabled = false;
    ctx.setTransform(this.scale, 0, 0, this.scale, this.offsetX, this.offsetY);
    if (this.staticLayer) ctx.drawImage(this.staticLayer, 0, 0);

    const entities = [...scene.entities.values()];
    const hitboxes: Hitbox[] = [];
    const focusKey = input.hoverKey ?? input.selectedKey;
    const focusAgent = focusKey ? agents[focusKey] : undefined;
    // The focused agent's team: its lead and every subagent of that lead.
    const familyRoot = focusAgent ? (focusAgent.parentKey ?? focusAgent.key) : null;

    // Lead → subagent links: always in a small office; in a busy one only
    // for the focused team (subagents already sit next to their lead).
    const allLinks = entities.length <= 16;
    ctx.lineWidth = 1;
    ctx.setLineDash([2, 2]);
    for (const e of entities) {
      const parent = e.parentKey ? scene.entities.get(e.parentKey) : undefined;
      if (!parent || (!allLinks && e.parentKey !== familyRoot)) continue;
      const agent = agents[e.key];
      ctx.strokeStyle = (agent && providers[agent.provider]?.descriptor.accentColor) || "#ffffff";
      ctx.globalAlpha = e.parentKey === familyRoot ? 0.9 : 0.45;
      ctx.beginPath();
      ctx.moveTo(parent.x, parent.y - 8);
      ctx.lineTo(e.x, e.y - 8);
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
    ctx.setLineDash([]);

    // Selection / attention rings under characters.
    for (const e of entities) {
      const selected = e.key === input.selectedKey;
      const alert = ATTENTION.has(e.activity);
      if (!selected && !alert && e.key !== input.hoverKey) continue;
      ctx.fillStyle = alert ? "#ff4d4d" : selected ? "#f2c94c" : "#ffffff";
      ctx.globalAlpha = alert ? 0.35 + 0.2 * Math.sin(now / 150) : selected ? 0.6 : 0.3;
      ctx.beginPath();
      ctx.ellipse(e.x, e.y + 1, 8, 3, 0, 0, Math.PI * 2);
      ctx.fill();
      ctx.globalAlpha = 1;
    }

    // Y-sorted furniture and characters.
    const drawables = this.drawables;
    drawables.length = 0;
    for (const f of this.layout.furniture) drawables.push({ y: (f.y + f.h) * TILE, furniture: f });
    for (const e of entities) drawables.push({ y: e.y, entity: e });
    drawables.sort((a, b) => a.y - b.y);
    for (const d of drawables) {
      if (d.furniture) {
        const f = d.furniture;
        const occupant = f.seatId ? scene.occupant(f.seatId) : undefined;
        const activity = occupant && !occupant.celebrating ? occupant.activity : null;
        drawFurniture(ctx, f, now, activity);
        continue;
      }
      const e = d.entity!;
      const agent = agents[e.key];
      const sprite = characterCanvas(this.look(e, agent, providers), frameFor(e, now));
      const typing = !e.path.length && e.sitting && e.activity === "CODING";
      const jump = e.celebrating ? Math.round(Math.abs(Math.sin(now / 130)) * 4) : 0;
      const bob = typing ? Math.floor(now / 180) % 2 : 0;
      const fading = e.fadeMs > 0;
      if (fading) ctx.globalAlpha = Math.max(0, 1 - e.fadeMs / EXIT_FADE_MS);
      if (sprite) ctx.drawImage(sprite, Math.round(e.x - SPRITE_W / 2), Math.round(e.y - (SPRITE_H - 1) - bob - jump));
      ctx.globalAlpha = 1;
      if (fading) drawPoof(ctx, e.x, e.y, Math.min(1, e.fadeMs / EXIT_FADE_MS));
      if (e.celebrating && agent?.endedAt !== undefined) drawConfetti(ctx, e.x, e.y, now - agent.endedAt, hashString(e.key));
      if (!fading) {
        hitboxes.push({
          key: e.key,
          kind: "agent",
          x: this.offsetX + (e.x - 8) * this.scale,
          y: this.offsetY + (e.y - 18) * this.scale,
          w: 16 * this.scale,
          h: 20 * this.scale,
        });
      }
    }

    // Bubbles: always for leads and for anyone who needs attention; for
    // other subagents only when their team is focused (keeps 70 agents calm).
    const withBubble = new Set<string>();
    for (const e of entities) {
      if (e.fadeMs > 0) continue;
      const agent = input.agents[e.key];
      const silent = !e.celebrating && !!input.sessions && silentSince(agent, input.sessions[agent?.sessionKey ?? ""]) !== null;
      const important = e.isMain || e.celebrating || silent || ATTENTION.has(e.activity);
      const focused = e.key === focusKey || (familyRoot !== null && (e.key === familyRoot || e.parentKey === familyRoot));
      if (!important && !focused) continue;
      const lift = e.celebrating ? Math.round(Math.abs(Math.sin(now / 130)) * 4) : 0;
      if (drawBubble(ctx, e, lift, now, silent)) withBubble.add(e.key);
    }

    // CEO inbox: pending approvals waiting for the user.
    const ceoDesk = this.layout.furniture.find((f) => f.kind === "ceoDesk");
    if (ceoDesk) {
      const ix = ceoDesk.x * TILE + 3;
      const iy = ceoDesk.y * TILE - 2;
      const papers = Math.min(input.pendingApprovals, 5);
      for (let i = 0; i < Math.max(papers, 1); i++) {
        ctx.fillStyle = i === 0 && papers === 0 ? "#d8d4ca" : "#f7f4ec";
        ctx.fillRect(ix, iy - i * 2, 10, 7);
        ctx.fillStyle = "#b9b2a4";
        ctx.fillRect(ix, iy - i * 2 + 6, 10, 1);
      }
      hitboxes.push({
        key: "ceo-inbox",
        kind: "inbox",
        x: this.offsetX + ceoDesk.x * TILE * this.scale,
        y: this.offsetY + (ceoDesk.y * TILE - 10) * this.scale,
        w: ceoDesk.w * TILE * this.scale,
        h: 26 * this.scale,
      });
    }

    // Screen-space text (crisp at any scale).
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.textBaseline = "middle";
    ctx.font = `600 ${Math.max(9, Math.round(5 * this.scale))}px "Segoe UI", system-ui, sans-serif`;
    for (const room of this.layout.rooms) {
      const x = this.offsetX + (room.rect.x + 1) * TILE * this.scale + 4;
      const y = this.offsetY + (room.rect.y + 1) * TILE * this.scale + 8;
      const text = room.name.toUpperCase();
      ctx.fillStyle = "rgba(15,17,23,0.55)";
      ctx.fillRect(x - 3, y - 7, this.measure(ctx, text) + 6, 14);
      ctx.fillStyle = "#e7e9ef";
      ctx.fillText(text, x, y);
    }

    // Team name plates on the desk rows.
    ctx.font = `700 ${Math.max(10, Math.round(5.5 * this.scale))}px "Segoe UI", system-ui, sans-serif`;
    const room = (id: string) => this.layout.rooms.find((r) => r.id === id)?.rect;
    for (const { pod, names } of scene.podLabels()) {
      const rect = room(pod.roomId);
      // The plate may use the floor up to the room's wall.
      const maxW = ((rect ? rect.x + rect.w - 1 : pod.label.x + 4) - pod.label.x) * TILE * this.scale - 4;
      let text = names.join(" · ");
      while (text.length > 1 && this.measure(ctx, text) + 12 > maxW) text = `${text.slice(0, -2)}…`;
      const x = this.offsetX + pod.label.x * TILE * this.scale;
      const y = this.offsetY + (pod.label.y * TILE + TILE / 2) * this.scale;
      const w = this.measure(ctx, text) + 12;
      ctx.fillStyle = "#3d2a1c";
      ctx.fillRect(x - 1, y - 10, w + 2, 20);
      ctx.fillStyle = "#8a5a33";
      ctx.fillRect(x, y - 9, w, 18);
      ctx.fillStyle = "#f2c94c";
      ctx.fillRect(x, y - 9, 3, 18);
      ctx.fillStyle = "#fff6e6";
      ctx.fillText(text, x + 7, y + 0.5);
    }

    if (ceoDesk && input.pendingApprovals > 0) {
      const x = this.offsetX + (ceoDesk.x * TILE + 14) * this.scale;
      const y = this.offsetY + (ceoDesk.y * TILE - 8) * this.scale;
      ctx.fillStyle = "#ff4d4d";
      ctx.beginPath();
      ctx.arc(x, y, 8, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = "#ffffff";
      ctx.textAlign = "center";
      ctx.fillText(String(input.pendingApprovals), x, y + 0.5);
      ctx.textAlign = "left";
    }

    // Name tags: leads always, subagents when few or focused.
    const labelAll = entities.length <= 16;
    ctx.font = `600 ${Math.max(10, Math.round(5.5 * this.scale))}px "Segoe UI", system-ui, sans-serif`;
    const placed: { l: number; t: number; r: number; b: number }[] = [];
    const labelled = entities
      .filter((e) => {
        const agent = agents[e.key];
        const focused = e.key === input.selectedKey || e.key === input.hoverKey;
        return agent && e.fadeMs === 0 && (agent.isMain || labelAll || focused);
      })
      .sort((a, b) => b.y - a.y);
    for (const e of labelled) {
      const agent = agents[e.key];
      const focused = e.key === input.selectedKey || e.key === input.hoverKey;
      const provider = providers[agent.provider]?.descriptor;
      const badge = provider?.badge ?? "?";
      const name = agent.name.length > 18 ? `${agent.name.slice(0, 17)}…` : agent.name;
      const cx = this.offsetX + e.x * this.scale;
      // Above the bubble when there is one, else just above the head.
      let cy = this.offsetY + (e.y - (withBubble.has(e.key) ? 36 : 23)) * this.scale;
      const badgeW = this.measure(ctx, badge) + 8;
      const nameW = this.measure(ctx, name) + 8;
      const total = badgeW + nameW;
      // Keep the tag on screen.
      const left = Math.round(Math.min(Math.max(cx - total / 2, 2), width - total - 2));
      cy = Math.max(cy, 10);
      // Nudge labels upwards until they no longer overlap one already drawn.
      for (let tries = 0; tries < 4; tries++) {
        const hit = placed.some((p) => left < p.r && left + total > p.l && cy - 8 < p.b && cy + 8 > p.t);
        if (!hit) break;
        cy -= 17;
      }
      placed.push({ l: left, t: cy - 8, r: left + total, b: cy + 8 });
      ctx.globalAlpha = agent.ended ? 0.6 : 1;
      ctx.fillStyle = provider?.accentColor ?? "#9aa3b5";
      ctx.fillRect(left, cy - 8, badgeW, 16);
      ctx.fillStyle = "#101218";
      ctx.fillText(badge, left + 4, cy);
      ctx.fillStyle = focused ? "rgba(242,201,76,0.95)" : "rgba(16,18,24,0.82)";
      ctx.fillRect(left + badgeW, cy - 8, nameW, 16);
      ctx.fillStyle = focused ? "#101218" : "#f3f4f7";
      ctx.fillText(name, left + badgeW + 4, cy);
      ctx.globalAlpha = 1;
    }
    return hitboxes;
  }
}
