// Canvas 2D renderer for the office. Static floors/walls are cached in an
// offscreen layer; furniture and characters are y-sorted every frame so
// seated characters are correctly occluded by their desks.

import type { Activity } from "../bindings/Activity";
import type { AgentState } from "../bindings/AgentState";
import type { ProviderInfo } from "../bindings/ProviderInfo";
import { type Furniture, type OfficeLayout, TILE } from "./layout";
import type { Entity, OfficeScene } from "./scene";
import { CHARACTER_FRAMES, type CharacterFrame, characterPalette, ICON_PALETTE, ICONS, spriteCanvas } from "./sprites";

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
}

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
  // Wall clock and poster decorations.
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

function drawFurniture(ctx: CanvasRenderingContext2D, f: Furniture, now: number, lit: Activity | null) {
  const x = f.x * TILE;
  const y = f.y * TILE;
  const w = f.w * TILE;
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
        ctx.fillStyle = "#2b2f3a";
        ctx.fillRect(x + 3, y - 3, 8, 6);
        ctx.fillStyle = Math.floor(now / 500) % 2 && lit ? "#3fbf5f" : "#2f8f4a";
        ctx.fillRect(x + 5, y - 1, 2, 2);
        ctx.fillStyle = "#9aa3b5";
        ctx.fillRect(x + 22, y + 2, 6, 3);
        break;
      }
      // Monitor (seen from the side/back) and keyboard.
      ctx.fillStyle = "#23262f";
      ctx.fillRect(x + 1, y - 7, 9, 9);
      ctx.fillStyle = "#3a3f4d";
      ctx.fillRect(x + 4, y + 2, 3, 2);
      if (lit) {
        const color = f.kind === "console" ? "#3fdc7a" : lit === "CODING" ? "#3bd1c6" : "#6aa8ff";
        ctx.fillStyle = color;
        ctx.globalAlpha = 0.55 + 0.25 * Math.sin(now / 240);
        ctx.fillRect(x + 2, y - 6, 7, 7);
        ctx.globalAlpha = 1;
      } else {
        ctx.fillStyle = "#30343f";
        ctx.fillRect(x + 2, y - 6, 7, 7);
      }
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

function frameFor(entity: Entity): CharacterFrame {
  if (entity.path.length) return Math.floor(entity.walkPhase / 140) % 2 ? "walkA" : "walkB";
  return entity.sitting ? "sit" : "stand";
}

function drawIcon(ctx: CanvasRenderingContext2D, name: string, x: number, y: number) {
  const sprite = spriteCanvas(ICONS[name], ICON_PALETTE, `icon:${name}`);
  if (sprite) ctx.drawImage(sprite, x, y);
}

function drawBubble(ctx: CanvasRenderingContext2D, entity: Entity, now: number) {
  const icon: Partial<Record<Activity, string>> = {
    READING: "doc",
    CODING: "code",
    RUNNING_COMMAND: "terminal",
    TESTING: "flask",
    WAITING_PERMISSION: "alert",
    WAITING_INPUT: "question",
    ERROR: "error",
    DONE: "check",
  };
  const activity = entity.activity;
  if (activity === "IDLE" && entity.zone !== "lounge") return;
  const waiting = activity === "WAITING_PERMISSION" || activity === "WAITING_INPUT";
  const bob = waiting ? Math.round(Math.abs(Math.sin(now / 160)) * -3) : 0;
  const bx = Math.round(entity.x + 3);
  const by = Math.round(entity.y - 30 + bob);
  ctx.fillStyle = activity === "WAITING_PERMISSION" || activity === "ERROR" ? "#fff1f1" : "#ffffff";
  ctx.fillRect(bx, by, 11, 10);
  ctx.fillStyle = "#2b2f3a";
  ctx.fillRect(bx, by - 1, 11, 1);
  ctx.fillRect(bx, by + 10, 11, 1);
  ctx.fillRect(bx - 1, by, 1, 10);
  ctx.fillRect(bx + 11, by, 1, 10);
  ctx.fillRect(bx + 1, by + 11, 2, 1);
  ctx.fillRect(bx, by + 12, 1, 1);
  if (activity === "THINKING") {
    const phase = Math.floor(now / 300) % 4;
    ctx.fillStyle = "#6b5bd6";
    for (let i = 0; i < 3; i++) if (i < phase) ctx.fillRect(bx + 2 + i * 3, by + 5, 2, 2);
    return;
  }
  const name = activity === "IDLE" ? "coffee" : icon[activity];
  if (name) drawIcon(ctx, name, bx + 2, by + 2);
}

export class OfficeRenderer {
  private staticLayer: HTMLCanvasElement | null = null;
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

    // Parent → subagent links.
    ctx.lineWidth = 1;
    ctx.setLineDash([2, 2]);
    for (const e of entities) {
      const agent = agents[e.key];
      const parent = agent?.parentKey ? scene.entities.get(agent.parentKey) : undefined;
      if (!agent || !parent) continue;
      ctx.strokeStyle = providers[agent.provider]?.descriptor.accentColor ?? "#ffffff";
      ctx.globalAlpha = 0.55;
      ctx.beginPath();
      ctx.moveTo(parent.x, parent.y - 8);
      ctx.lineTo(e.x, e.y - 8);
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
    ctx.setLineDash([]);

    // Selection / alert rings under characters.
    for (const e of entities) {
      const selected = e.key === input.selectedKey;
      const alert = e.activity === "WAITING_PERMISSION" || e.activity === "WAITING_INPUT" || e.activity === "ERROR";
      if (!selected && !alert && e.key !== input.hoverKey) continue;
      ctx.fillStyle = alert ? "#ff4d4d" : selected ? "#f2c94c" : "#ffffff";
      ctx.globalAlpha = alert ? 0.35 + 0.2 * Math.sin(now / 150) : selected ? 0.6 : 0.3;
      ctx.beginPath();
      ctx.ellipse(e.x, e.y + 1, 8, 3, 0, 0, Math.PI * 2);
      ctx.fill();
      ctx.globalAlpha = 1;
    }

    // Y-sorted furniture and characters.
    type Drawable = { y: number; draw: () => void };
    const drawables: Drawable[] = [];
    for (const f of this.layout.furniture) {
      const occupant = f.seatId ? scene.occupant(f.seatId) : undefined;
      const lit = occupant && occupant.activity !== "IDLE" && occupant.activity !== "DONE" ? occupant.activity : null;
      drawables.push({ y: (f.y + f.h) * TILE, draw: () => drawFurniture(ctx, f, now, lit) });
    }
    for (const e of entities) {
      const agent = agents[e.key];
      const accent = (agent && providers[agent.provider]?.descriptor.accentColor) || "#9aa3b5";
      drawables.push({
        y: e.y,
        draw: () => {
          const frame = frameFor(e);
          const sprite = spriteCanvas(CHARACTER_FRAMES[frame], characterPalette(e.key, accent), `${e.key}:${accent}:${frame}`);
          const typing = !e.path.length && (e.activity === "CODING" || e.activity === "RUNNING_COMMAND");
          const bob = typing ? Math.floor(now / 180) % 2 : 0;
          if (sprite) ctx.drawImage(sprite, Math.round(e.x - 6), Math.round(e.y - 15 - bob));
        },
      });
      hitboxes.push({
        key: e.key,
        kind: "agent",
        x: this.offsetX + (e.x - 8) * this.scale,
        y: this.offsetY + (e.y - 18) * this.scale,
        w: 16 * this.scale,
        h: 20 * this.scale,
      });
    }
    drawables.sort((a, b) => a.y - b.y);
    for (const d of drawables) d.draw();

    for (const e of entities) drawBubble(ctx, e, now);

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
        x: this.offsetX + (ceoDesk.x * TILE) * this.scale,
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
      ctx.fillRect(x - 3, y - 7, ctx.measureText(text).width + 6, 14);
      ctx.fillStyle = "#e7e9ef";
      ctx.fillText(text, x, y);
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

    const labelAll = entities.length <= 16;
    ctx.font = `600 ${Math.max(10, Math.round(5.5 * this.scale))}px "Segoe UI", system-ui, sans-serif`;
    const placed: { l: number; t: number; r: number; b: number }[] = [];
    const labelled = entities
      .filter((e) => {
        const agent = agents[e.key];
        const focused = e.key === input.selectedKey || e.key === input.hoverKey;
        return agent && (agent.isMain || labelAll || focused);
      })
      .sort((a, b) => b.y - a.y);
    for (const e of labelled) {
      const agent = agents[e.key];
      const focused = e.key === input.selectedKey || e.key === input.hoverKey;
      const provider = providers[agent.provider]?.descriptor;
      const badge = provider?.badge ?? "?";
      const name = agent.name.length > 18 ? `${agent.name.slice(0, 17)}…` : agent.name;
      const cx = this.offsetX + e.x * this.scale;
      let cy = this.offsetY + (e.y - 36) * this.scale;
      const badgeW = ctx.measureText(badge).width + 8;
      const nameW = ctx.measureText(name).width + 8;
      const total = badgeW + nameW;
      const left = Math.round(cx - total / 2);
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
