// Canvas 2D renderer for the office. Static floors/walls are cached in an
// offscreen layer; furniture and characters are y-sorted every frame so
// seated characters are correctly occluded by their desks. Character frames
// are cached per look (see sprites.ts), so a frame costs a few drawImage
// calls per character.

import type { Activity } from "../bindings/Activity";
import type { AgentState } from "../bindings/AgentState";
import type { ProviderInfo } from "../bindings/ProviderInfo";
import type { SessionState } from "../bindings/SessionState";
import type { Appearance } from "../state/appearance";
import { silentSince } from "../state/silence";
import { ART, type Furniture, type Grid, type OfficeLayout, TILE } from "./layout";
import { OfficePet, PET_FRAMES, PET_PALETTE } from "./pet";
import { EXIT_FADE_MS, type Entity, type OfficeScene } from "./scene";
import {
  type CharacterFrame,
  type CharacterLook,
  characterCanvas,
  characterLook,
  hashString,
  ICON_PALETTE,
  ICONS,
  scale2x,
  SPRITE_H,
  SPRITE_W,
  spriteCanvas,
} from "./sprites";

export interface Hitbox {
  key: string;
  kind: "agent" | "inbox" | "offscreen";
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
  /** The user's looks per team (optional). */
  appearance?: Record<string, Appearance>;
  /** Which office to paint (modern when omitted). */
  theme?: OfficeTheme;
  /** Who stays fully visible; everyone else is dimmed (all when omitted). */
  filter?: OfficeFilter;
}

export type OfficeFilter = { kind: "attention" } | { kind: "team"; team: string };

const ATTENTION: ReadonlySet<Activity> = new Set(["WAITING_PERMISSION", "WAITING_INPUT", "ERROR"]);

/** Polished concrete: soft slab tones, a little aggregate, a seam every four tiles. */
function concrete(ctx: CanvasRenderingContext2D, x: number, y: number, base: string, light: string, dark: string, seam: string) {
  ctx.fillStyle = base;
  ctx.fillRect(x, y, TILE, TILE);
  // Fine aggregate: half-pixel specks, light and dark.
  for (let i = 0; i < 6; i++) {
    const n = hashString(`${x},${y},${i}`);
    ctx.fillStyle = i % 2 ? dark : light;
    ctx.fillRect(x + (n % 31) / 2, y + ((n >>> 5) % 31) / 2, 0.5, 0.5);
  }
  ctx.fillStyle = seam;
  if ((x / TILE) % 4 === 0) ctx.fillRect(x, y, 0.5, TILE);
  if ((y / TILE) % 4 === 0) ctx.fillRect(x, y, TILE, 0.5);
}

/** Long boards in staggered rows. */
function planks(ctx: CanvasRenderingContext2D, x: number, y: number, base: string, alt: string, seam: string) {
  for (let row = 0; row < 4; row++) {
    const n = hashString(`${x},${y},${row}`);
    ctx.fillStyle = n % 3 ? base : alt;
    ctx.fillRect(x, y + row * 4, TILE, 4);
    ctx.fillStyle = seam;
    ctx.fillRect(x, y + row * 4 + 3.5, TILE, 0.5);
    ctx.fillRect(x + ((y / TILE + row) % 2 ? 4 : 12), y + row * 4, 0.5, 3.5);
    // Grain: a faint streak along the board.
    ctx.fillStyle = "rgba(0,0,0,0.08)";
    ctx.fillRect(x + (n % 8), y + row * 4 + 1.5, 5 + ((n >>> 3) % 6), 0.5);
  }
}

type Painter = (ctx: CanvasRenderingContext2D, x: number, y: number) => void;

const MODERN_FLOORS: Record<string, Painter> = {
  // CEO office: walnut boards.
  wood(ctx, x, y) {
    planks(ctx, x, y, "#8a5f3d", "#805837", "#6c4a2f");
  },
  // Meeting room: charcoal carpet tiles laid in alternating directions.
  carpetBlue(ctx, x, y) {
    ctx.fillStyle = "#3d4250";
    ctx.fillRect(x, y, TILE, TILE);
    ctx.fillStyle = "#454b5a";
    const across = (x / TILE + y / TILE) % 2 === 0;
    for (let i = 1; i < TILE; i += 1.5) {
      if (across) ctx.fillRect(x, y + i, TILE, 0.5);
      else ctx.fillRect(x + i, y, 0.5, TILE);
    }
  },
  // QA lab: light grey resin with fine seams.
  checker(ctx, x, y) {
    concrete(ctx, x, y, "#a9b0bc", "#b5bcc7", "#9da4b0", "#98a0ac");
  },
  // Open space: polished concrete.
  carpet(ctx, x, y) {
    concrete(ctx, x, y, "#676c78", "#727783", "#5d626e", "#5a5f6b");
  },
  // Terminal room: raised-floor tiles with vents.
  metal(ctx, x, y) {
    ctx.fillStyle = "#353a46";
    ctx.fillRect(x, y, TILE, TILE);
    ctx.fillStyle = "#2c303a";
    ctx.fillRect(x, y + TILE - 1, TILE, 1);
    ctx.fillRect(x + TILE - 1, y, 1, TILE);
    ctx.fillStyle = "#2f3440";
    for (let i = 4; i < 12; i += 2) ctx.fillRect(x + 4, y + i, 8, 1);
  },
  // Lounge: light oak boards.
  warm(ctx, x, y) {
    planks(ctx, x, y, "#b08a5e", "#a78256", "#94714a");
  },
  corridor(ctx, x, y) {
    concrete(ctx, x, y, "#7d818b", "#878b95", "#737781", "#6e727c");
  },
};


/** Cyberpunk floors: metal grating, wet asphalt, black marble, neon specks. */
const CYBER_FLOORS: Record<string, Painter> = {
  // CEO office: black marble with magenta veins.
  wood(ctx, x, y) {
    ctx.fillStyle = "#1b1722";
    ctx.fillRect(x, y, TILE, TILE);
    const n = hashString(`${x},${y}`);
    ctx.fillStyle = "rgba(255,43,214,0.22)";
    for (let i = 0; i < 6; i++) ctx.fillRect(x + ((n >>> i) % 10) + i, y + i * 2 + (n % 3), 0.5, 0.5);
    ctx.fillStyle = "#1c1824";
    ctx.fillRect(x, y, TILE, 0.5);
    ctx.fillRect(x, y, 0.5, TILE);
  },
  // Meeting room: navy floor with a cyan dot grid.
  carpetBlue(ctx, x, y) {
    ctx.fillStyle = "#181d36";
    ctx.fillRect(x, y, TILE, TILE);
    ctx.fillStyle = "rgba(0,229,255,0.3)";
    for (let i = 2; i < TILE; i += 4) for (let j = 2; j < TILE; j += 4) ctx.fillRect(x + i, y + j, 0.5, 0.5);
  },
  // QA lab: dark teal bricks with a green status light here and there.
  checker(ctx, x, y) {
    ctx.fillStyle = "#13252c";
    ctx.fillRect(x, y, TILE, TILE);
    ctx.fillStyle = "#17323b";
    for (let j = 0; j < TILE; j += 4) {
      ctx.fillRect(x, y + j, TILE, 0.5);
      for (let i = (j / 4) % 2 ? 4 : 0; i < TILE; i += 8) ctx.fillRect(x + i, y + j, 0.5, 4);
    }
    if (hashString(`${x},${y}`) % 7 === 0) {
      ctx.fillStyle = "#00ff9f";
      ctx.fillRect(x + 6, y + 6, 0.5, 0.5);
    }
  },
  // Open space: metal grating with bolts.
  carpet(ctx, x, y) {
    ctx.fillStyle = "#1e1c36";
    ctx.fillRect(x, y, TILE, TILE);
    ctx.fillStyle = "#2a2750";
    for (let i = 0; i < TILE; i += 4) {
      ctx.fillRect(x + i, y, 0.5, TILE);
      ctx.fillRect(x, y + i, TILE, 0.5);
    }
    ctx.fillStyle = "#2c2950";
    ctx.fillRect(x + 0.5, y + 0.5, 0.5, 0.5);
    const n = hashString(`${x},${y}`);
    if (n % 5 === 0) {
      ctx.fillStyle = "rgba(0,229,255,0.4)";
      ctx.fillRect(x + (n % 30) / 2, y + ((n >>> 5) % 30) / 2, 0.5, 0.5);
    }
  },
  // Terminal room: dark plates with cyan-lit vents.
  metal(ctx, x, y) {
    ctx.fillStyle = "#1d1c27";
    ctx.fillRect(x, y, TILE, TILE);
    ctx.fillStyle = "#13121a";
    ctx.fillRect(x, y + TILE - 0.5, TILE, 0.5);
    ctx.fillRect(x + TILE - 0.5, y, 0.5, TILE);
    for (let i = 4; i < 12; i += 1.5) {
      ctx.fillStyle = "#0f0e15";
      ctx.fillRect(x + 4, y + i, 8, 0.5);
      ctx.fillStyle = "rgba(0,229,255,0.18)";
      ctx.fillRect(x + 4, y + i + 0.5, 8, 0.5);
    }
  },
  // Lounge: deep purple carpet with a diamond weave.
  warm(ctx, x, y) {
    ctx.fillStyle = "#2e1639";
    ctx.fillRect(x, y, TILE, TILE);
    ctx.fillStyle = "#3d1e4c";
    for (let i = 0; i < 8; i++) {
      ctx.fillRect(x + 8 - i, y + i, 0.5, 0.5);
      ctx.fillRect(x + 8 + i, y + i, 0.5, 0.5);
      ctx.fillRect(x + i, y + 8 + i, 0.5, 0.5);
      ctx.fillRect(x + 15.5 - i, y + 8 + i, 0.5, 0.5);
    }
  },
  // Corridors: wet asphalt with neon puddles.
  corridor(ctx, x, y) {
    concrete(ctx, x, y, "#1a1924", "#24232f", "#121119", "#15141d");
    const n = hashString(`puddle:${x},${y}`);
    if (n % 4 === 0) {
      ctx.fillStyle = n % 8 ? "rgba(255,43,214,0.16)" : "rgba(0,229,255,0.16)";
      ctx.fillRect(x + 2 + (n % 6), y + 5 + ((n >>> 3) % 6), 7, 2);
      ctx.fillStyle = "rgba(255,255,255,0.12)";
      ctx.fillRect(x + 3 + (n % 6), y + 5.5 + ((n >>> 3) % 6), 3, 0.5);
    }
  },
};


/** L.A. Noire floors: worn linoleum, terrazzo halls, bare concrete, lab tile. */
const NOIR_FLOORS: Record<string, Painter> = {
  // Captain's office: dark oak parquet.
  wood(ctx, x, y) {
    planks(ctx, x, y, "#4a3322", "#43301f", "#2e1f14");
  },
  // Interrogation room: bare concrete.
  carpetBlue(ctx, x, y) {
    concrete(ctx, x, y, "#3c3c3a", "#474744", "#333331", "#2f2f2d");
  },
  // Crime lab: small white tiles, grey grout.
  checker(ctx, x, y) {
    ctx.fillStyle = "#8a867c";
    ctx.fillRect(x, y, TILE, TILE);
    ctx.fillStyle = "#65625a";
    for (let i = 0; i < TILE; i += 4) {
      ctx.fillRect(x + i, y, 0.5, TILE);
      ctx.fillRect(x, y + i, TILE, 0.5);
    }
  },
  // Detective bullpen: worn linoleum checks with scuffs.
  carpet(ctx, x, y) {
    for (let i = 0; i < 2; i++)
      for (let j = 0; j < 2; j++) {
        ctx.fillStyle = (i + j) % 2 ? "#3a332b" : "#2f2923";
        ctx.fillRect(x + i * 8, y + j * 8, 8, 8);
      }
    const n = hashString(`${x},${y}`);
    ctx.fillStyle = "rgba(0,0,0,0.18)";
    ctx.fillRect(x + (n % 10), y + ((n >>> 4) % 12), 4 + (n % 3), 0.5);
  },
  // Records room: old dark boards.
  metal(ctx, x, y) {
    planks(ctx, x, y, "#3a2a1c", "#352619", "#24180f");
  },
  // Lounge: a burgundy rug with a border.
  warm(ctx, x, y) {
    ctx.fillStyle = "#4a1f1f";
    ctx.fillRect(x, y, TILE, TILE);
    ctx.fillStyle = "#5a2a24";
    ctx.fillRect(x + 2, y + 2, 12, 0.5);
    ctx.fillRect(x + 2, y + 13.5, 12, 0.5);
    ctx.fillStyle = "#6b3a2a";
    ctx.fillRect(x + 7.5, y + 7.5, 1, 1);
  },
  // Halls: terrazzo with flecks.
  corridor(ctx, x, y) {
    concrete(ctx, x, y, "#4a4540", "#57514a", "#3e3a35", "#3a3631");
  },
};

/** Everything the office world paints that changes with the theme. */
interface WorldTheme {
  /** Behind the office on screen, and outside the building. */
  backdrop: string;
  void: string;
  floors: Record<string, Painter>;
  /** Night dimming "r,g,b" and its darkest alpha; lamp tints "r,g,b" picked per room. */
  night: string;
  nightAlpha: number;
  lamps: string[];
  lampAlpha: number;
  chair: { frame: string; meshA: string; meshB: string; trim: string };
  glass: { top: string; pane: string; bottom: string; shine: string; post: string };
  wall: { body: string; cap: string; edge: string; base: string; trim?: string };
  sky: { top: string; low: string; star: string; tower: string; lights: string[]; head: string; mullion: string; sill: string };
  desk: { top: string; edge: string; shine: string; legs: string; bezel: string; stand: string; kbd: string; keys: string[]; keyRow: string; mug: string; trim?: string };
  console: { top: string; edge: string; shine: string; trim?: string };
  benchTrim?: string;
  rack: { body: string; panel: string; leds: [string, string]; off: string };
  /** Pieces whose shape changes with the theme (the default painter otherwise). */
  furniture: Partial<Record<Furniture["kind"], (ctx: CanvasRenderingContext2D, f: Furniture, now: number) => void>>;
  decor: (ctx: CanvasRenderingContext2D, kind: string, x: number, y: number) => void;
  neon: { halo: string; core: string; glow: string };
  plate: { shadow: string; body: string; bar: string; text: string };
  mat: string;
  /** Rain on the outer glazing and flickering neon (drawn every frame). */
  weather: boolean;
  /** Venetian-blind light cast in slanted bands below the windows. */
  blinds?: boolean;
  /** Darkened edges over the office, like an old film frame. */
  vignette?: boolean;
  /** Follows the local time of day (otherwise it is always night). */
  daylight?: boolean;
}

export type DayPhase = "dawn" | "day" | "dusk" | "night";

/** The phase of the day for a local hour. */
export function dayPhase(hour: number): DayPhase {
  if (hour >= 7 && hour < 17) return "day";
  if (hour >= 17 && hour < 20) return "dusk";
  if (hour >= 6 && hour < 7) return "dawn";
  return "night";
}

/** A daylight theme repainted for a phase: less dimming, a sky to match, city lights off by day. */
function atPhase(theme: WorldTheme, phase: DayPhase): WorldTheme {
  if (!theme.daylight || phase === "night") return theme;
  const day = phase === "day";
  return {
    ...theme,
    nightAlpha: theme.nightAlpha * (day ? 0.12 : 0.6),
    lampAlpha: theme.lampAlpha * (day ? 0.3 : 1),
    sky: day
      ? { ...theme.sky, top: "#6fa8dc", low: "#a9cbe8", star: "#ffffff", tower: "#7d8fa6", lights: ["#c9d6e6"] }
      : { ...theme.sky, top: "#3a2a5a", low: "#d9825a", star: "#ffd8a8", tower: "#2a2238", lights: ["#f2c94c", "#ffd8a8"] },
  };
}

export type OfficeTheme = "modern" | "cyberpunk" | "noire";

const UNLIT: ReadonlySet<string> = new Set(["plant", "serverRack", "waterCooler", "whiteboard", "bookshelf"]);

/** Which space a tile belongs to for lighting: its room, the corridors, or nothing outside. */
function spaceOf(grid: Grid, tx: number, ty: number): string | null {
  const i = ty * grid.width + tx;
  return grid.cells[i] === "corridor" ? "corridor" : grid.roomAt[i];
}

type Lamp = { x: number; y: number; r: number; space: string | null };

/** Where the light comes from at night: lamps over the furniture people use, and the corridors (layout px, radius). */
function lampSpots(layout: OfficeLayout, grid: Grid): Lamp[] {
  const spots: Lamp[] = [];
  for (const f of layout.furniture) {
    if (UNLIT.has(f.kind)) continue;
    spots.push({ x: (f.x + f.w / 2) * TILE, y: (f.y + f.h / 2 - 0.5) * TILE, r: TILE * Math.max(2.6, f.w * 0.55 + 1.8), space: spaceOf(grid, f.x, f.y) });
  }
  for (const c of layout.corridors) {
    const horizontal = c.w >= c.h;
    const len = horizontal ? c.w : c.h;
    for (let k = 3; k < len; k += 6) {
      const x = horizontal ? c.x + k : c.x + c.w / 2;
      const y = horizontal ? c.y + c.h / 2 : c.y + k;
      spots.push({ x: x * TILE, y: y * TILE, r: TILE * 2.4, space: "corridor" });
    }
  }
  return spots;
}

/**
 * Dims the floors to night and lights them around the lamps, in 1 px blocks
 * and four steps so the light is stepped like the rest of the pixel art. A
 * lamp only lights its own room (or the corridors); outside stays dark.
 */
function drawNight(ctx: CanvasRenderingContext2D, layout: OfficeLayout, grid: Grid, width: number, height: number) {
  const B = 1;
  const STEPS = 4;
  const bySpace = new Map<string | null, Lamp[]>();
  for (const s of lampSpots(layout, grid)) bySpace.set(s.space, [...(bySpace.get(s.space) ?? []), s]);
  for (let y = 0; y < height; y += B) {
    for (let x = 0; x < width; x += B) {
      const space = spaceOf(grid, Math.floor(x / TILE), Math.floor(y / TILE));
      let light = 0;
      if (space !== null) {
        for (const s of bySpace.get(space) ?? []) {
          const d = Math.hypot(x + B / 2 - s.x, (y + B / 2 - s.y) * 1.25) / s.r;
          if (d < 1) light = Math.max(light, 1 - d);
        }
      }
      const level = Math.min(STEPS, Math.ceil(light * STEPS * 1.15)) / STEPS;
      ctx.fillStyle = `rgba(${T.night},${(T.nightAlpha * (1 - 0.82 * level)).toFixed(3)})`;
      ctx.fillRect(x, y, B, B);
      if (level > 0 && space !== null) {
        ctx.fillStyle = `rgba(${T.lamps[hashString(space) % T.lamps.length]},${(T.lampAlpha * level).toFixed(3)})`;
        ctx.fillRect(x, y, B, B);
      }
    }
  }
}


/**
 * Venetian blinds: under each outer window, slanted bands of light fall
 * across the floor and fade with distance, staying inside the room.
 */
function drawBlinds(ctx: CanvasRenderingContext2D, layout: OfficeLayout, grid: Grid, cellAt: (tx: number, ty: number) => string) {
  for (const [tx, ty] of windowTiles(layout, cellAt)) {
    const room = spaceOf(grid, tx, ty + 1);
    const top = (ty + 1) * TILE;
    // Nine slats of light, each 2 px, fading as they reach into the room.
    for (let band = 0; band < 9; band++) {
      ctx.fillStyle = `rgba(255,214,150,${(0.22 * (1 - band / 10)).toFixed(3)})`;
      for (let yy = top + 1 + band * 4; yy < top + 3 + band * 4; yy += 0.5) {
        const sx = tx * TILE + 1 + (yy - top) * 0.6;
        const cx = Math.floor((sx + 7) / TILE);
        const cy = Math.floor(yy / TILE);
        if (cellAt(cx, cy) !== "floor" || spaceOf(grid, cx, cy) !== room) continue;
        ctx.fillRect(sx, yy, 14, 0.5);
      }
    }
  }
}

const CHAIR_ZONES: ReadonlySet<string> = new Set(["desk", "qa", "terminal", "meeting"]);

/** Office chairs behind every seat where someone sits to work; an occupant covers them. */
function drawChairs(ctx: CanvasRenderingContext2D, layout: OfficeLayout) {
  for (const seat of layout.seats) {
    if (!seat.sitting || !CHAIR_ZONES.has(seat.zone)) continue;
    const x = Math.round(seat.x * TILE + TILE / 2 + (seat.dx ?? 0)) - 6;
    const y = seat.y * TILE + TILE - 1 + (seat.dy ?? 0) - 14;
    // Ergonomic mesh chair: frame, mesh back, headrest and armrests.
    ctx.fillStyle = "rgba(0,0,0,0.25)";
    ctx.fillRect(x + 1, y + 10, 11, 2);
    ctx.fillStyle = T.chair.frame;
    ctx.fillRect(x + 3, y - 3, 6, 3);
    ctx.fillRect(x, y, 12, 10);
    for (let row = 1; row < 9; row += 0.5) {
      ctx.fillStyle = (row * 2) % 2 ? T.chair.meshA : T.chair.meshB;
      ctx.fillRect(x + 1, y + row, 10, 0.5);
    }
    ctx.fillStyle = T.chair.trim;
    ctx.fillRect(x + 4, y - 2, 4, 1);
    ctx.fillRect(x - 1, y + 5, 1, 4);
    ctx.fillRect(x + 12, y + 5, 1, 4);
  }
}

/**
 * A glass partition seen from above: a thin pane between aluminium rails,
 * running along the wall (both ways at a junction, with a post).
 */
function drawGlass(ctx: CanvasRenderingContext2D, tx: number, ty: number, cellAt: (tx: number, ty: number) => string) {
  const x = tx * TILE;
  const y = ty * TILE;
  const joins = (cx: number, cy: number) => cellAt(cx, cy) === "wall";
  const across = joins(tx - 1, ty) || joins(tx + 1, ty);
  const down = joins(tx, ty - 1) || joins(tx, ty + 1);
  if (across || !down) {
    ctx.fillStyle = "rgba(0,0,0,0.18)";
    ctx.fillRect(x, y + 11, TILE, 1);
    ctx.fillStyle = T.glass.top;
    ctx.fillRect(x, y + 6.5, TILE, 0.5);
    ctx.fillStyle = T.glass.pane;
    ctx.fillRect(x, y + 7, TILE, 3);
    ctx.fillStyle = T.glass.bottom;
    ctx.fillRect(x, y + 10, TILE, 0.5);
    ctx.fillStyle = T.glass.shine;
    if (tx % 2) ctx.fillRect(x + 4, y + 7.5, 4, 0.5);
  }
  if (down) {
    ctx.fillStyle = "rgba(0,0,0,0.18)";
    ctx.fillRect(x + 11, y, 1, TILE);
    ctx.fillStyle = T.glass.top;
    ctx.fillRect(x + 6.5, y, 0.5, TILE);
    ctx.fillStyle = T.glass.pane;
    ctx.fillRect(x + 7, y, 3, TILE);
    ctx.fillStyle = T.glass.bottom;
    ctx.fillRect(x + 10, y, 0.5, TILE);
    ctx.fillStyle = T.glass.shine;
    if (ty % 2) ctx.fillRect(x + 7.5, y + 4, 0.5, 4);
  }
  if (across && down) {
    ctx.fillStyle = T.glass.post;
    ctx.fillRect(x + 6, y + 6, 5, 5);
  }
}

/** Outer-wall tiles that carry glazing: on the building's top edge, above a room, not behind a fitting. */
function windowTiles(layout: OfficeLayout, cellAt: (tx: number, ty: number) => string): [number, number][] {
  const covered = (tx: number, ty: number) =>
    layout.furniture.some((f) => tx >= f.x && tx < f.x + f.w && ty >= f.y && ty < f.y + f.h) ||
    layout.decorations.some((d) => d.x === tx && d.y === ty);
  const tiles: [number, number][] = [];
  for (let ty = 0; ty < layout.height; ty++)
    for (let tx = 0; tx < layout.width; tx++)
      if (cellAt(tx, ty) === "wall" && cellAt(tx, ty - 1) === "void" && cellAt(tx, ty + 1) === "floor" && !covered(tx, ty) && !covered(tx, ty + 1))
        tiles.push([tx, ty]);
  return tiles;
}

/** Floor-to-ceiling glazing on the outer wall: night sky, a star, the lights of the city. */
function drawWindows(ctx: CanvasRenderingContext2D, layout: OfficeLayout, cellAt: (tx: number, ty: number) => string) {
  for (const [tx, ty] of windowTiles(layout, cellAt)) {
    {
      const x = tx * TILE;
      const y = ty * TILE;
      const n = hashString(`window:${tx}`);
      ctx.fillStyle = T.sky.top;
      ctx.fillRect(x, y + 4, TILE, 9);
      ctx.fillStyle = T.sky.low;
      ctx.fillRect(x, y + 9, TILE, 4);
      ctx.fillStyle = T.sky.star;
      ctx.fillRect(x + 1 + (n % 14), y + 5 + ((n >>> 4) % 3), 0.5, 0.5);
      // Distant towers with a few lit windows.
      ctx.fillStyle = T.sky.tower;
      ctx.fillRect(x + 1 + ((n >>> 6) % 8), y + 9 - ((n >>> 9) % 3), 4, 4 + ((n >>> 9) % 3));
      ctx.fillRect(x + 8 + ((n >>> 2) % 5), y + 7 - ((n >>> 11) % 3), 3, 6 + ((n >>> 11) % 3));
      for (let k = 0; k < 4; k++) {
        ctx.fillStyle = T.sky.lights[(n >>> k) % T.sky.lights.length];
        ctx.fillRect(x + 1 + ((n >>> (8 + k * 3)) % 14), y + 9.5 + ((n >>> (k * 5)) % 3), 0.5, 0.5);
      }
      ctx.fillStyle = T.sky.head;
      ctx.fillRect(x, y + 3, TILE, 1);
      ctx.fillStyle = T.sky.mullion;
      ctx.fillRect(x, y + 4, 0.5, 9);
      ctx.fillStyle = T.sky.sill;
      ctx.fillRect(x, y + 13, TILE, 0.5);
    }
  }
}

const GLOW: Partial<Record<Activity, string>> = {
  CODING: "106,168,255",
  THINKING: "106,168,255",
  READING: "238,241,246",
  RUNNING_COMMAND: "63,220,122",
  TESTING: "63,220,122",
  WAITING_PERMISSION: "255,92,92",
  ERROR: "255,92,92",
  WAITING_INPUT: "242,153,74",
};

const glows = new Map<string, HTMLCanvasElement | null>();

/** A soft oval of screen light, cached per color (r,g,b). */
function glowSprite(rgb: string): HTMLCanvasElement | null {
  if (glows.has(rgb)) return glows.get(rgb) ?? null;
  let canvas: HTMLCanvasElement | null = null;
  if (typeof document !== "undefined") {
    canvas = document.createElement("canvas");
    canvas.width = 44;
    canvas.height = 28;
    const g = canvas.getContext("2d");
    if (g) {
      g.scale(1, 28 / 44);
      const grad = g.createRadialGradient(22, 22, 0, 22, 22, 22);
      grad.addColorStop(0, `rgba(${rgb},0.24)`);
      grad.addColorStop(1, `rgba(${rgb},0)`);
      g.fillStyle = grad;
      g.fillRect(0, 0, 44, 44);
    } else canvas = null;
  }
  glows.set(rgb, canvas);
  return canvas;
}

// ---------------------------------------------------------------- themes

function modernDecor(ctx: CanvasRenderingContext2D, kind: string, x: number, y: number) {
  if (kind === "clock") {
    ctx.fillStyle = "#e9e4d8";
    ctx.fillRect(x + 4, y + 4, 8, 8);
    ctx.fillStyle = "#2b2f3a";
    ctx.fillRect(x + 7, y + 5, 1, 4);
    ctx.fillRect(x + 8, y + 8, 3, 1);
  } else {
    // Framed abstract print.
    ctx.fillStyle = "#1b1d24";
    ctx.fillRect(x + 2, y + 2, 12, 11);
    ctx.fillStyle = "#e9ecf1";
    ctx.fillRect(x + 3, y + 3, 10, 9);
    ctx.fillStyle = "#d97757";
    ctx.fillRect(x + 4, y + 7, 5, 4);
    ctx.fillStyle = "#4c8df5";
    ctx.fillRect(x + 8, y + 4, 4, 4);
  }
}

function cyberDecor(ctx: CanvasRenderingContext2D, kind: string, x: number, y: number) {
  if (kind === "clock") {
    // Red digital clock.
    ctx.fillStyle = "#07060c";
    ctx.fillRect(x + 2, y + 5, 12, 6);
    ctx.fillStyle = "#ff3b5c";
    for (const dx of [3, 6, 9.5, 12]) {
      ctx.fillRect(x + dx, y + 6, 1.5, 0.5);
      ctx.fillRect(x + dx, y + 9.5, 1.5, 0.5);
      ctx.fillRect(x + dx + 1, y + 6, 0.5, 4);
    }
    ctx.fillRect(x + 8.5, y + 7, 0.5, 0.5);
    ctx.fillRect(x + 8.5, y + 8.5, 0.5, 0.5);
  } else {
    // Holographic ad panel.
    ctx.fillStyle = "#00e5ff";
    ctx.fillRect(x + 2, y + 2, 12, 11);
    ctx.fillStyle = "#1a0b2e";
    ctx.fillRect(x + 2.5, y + 2.5, 11, 10);
    ctx.fillStyle = "#ff2bd6";
    ctx.fillRect(x + 4, y + 4, 4, 4);
    ctx.fillStyle = "#7c4dff";
    ctx.fillRect(x + 7, y + 7, 5, 4);
    ctx.fillStyle = "rgba(0,229,255,0.5)";
    for (let j = 3; j < 12; j += 1) ctx.fillRect(x + 2.5, y + j, 11, 0.5);
  }
}

/** Cyberpunk pieces: same footprint as their modern counterparts, different object. */
const CYBER_FURNITURE: WorldTheme["furniture"] = {
  ceoDesk(ctx, f) {
    const x = f.x * TILE;
    const y = f.y * TILE;
    const w = f.w * TILE;
    // Black slab with a gold edge and a floating holo display.
    ctx.fillStyle = "#0d0c14";
    ctx.fillRect(x, y + 1, w, 10);
    ctx.fillStyle = "#24203a";
    ctx.fillRect(x, y + 1, w, 0.5);
    ctx.fillStyle = "#f2c94c";
    ctx.fillRect(x, y + 10.5, w, 0.5);
    ctx.fillStyle = "rgba(0,229,255,0.28)";
    ctx.fillRect(x + 29, y - 9, 16, 9);
    ctx.fillStyle = "#00e5ff";
    ctx.fillRect(x + 29, y - 9, 16, 0.5);
    for (let j = 0; j < 4; j++) ctx.fillRect(x + 31, y - 7 + j * 2, 4 + ((j * 5) % 9), 0.5);
    ctx.fillStyle = "#ff2bd6";
    ctx.fillRect(x + 14, y + 3, 10, 5);
    ctx.fillStyle = "#1b1a2a";
    ctx.fillRect(x + 14.5, y + 3.5, 9, 4);
  },
  meetingTable(ctx, f, now) {
    const x = f.x * TILE;
    const y = f.y * TILE;
    const w = f.w * TILE;
    const h = f.h * TILE;
    // Black glass table with cyan rails and a hologram at its centre.
    ctx.fillStyle = "#10101c";
    ctx.fillRect(x + 4, y + 3, w - 8, h - 8);
    ctx.fillRect(x + 2, y + 5, w - 4, h - 12);
    ctx.fillStyle = "#00e5ff";
    ctx.fillRect(x + 4, y + 3, w - 8, 0.5);
    ctx.fillRect(x + 4, y + h - 5.5, w - 8, 0.5);
    for (const lx of [x + 20, x + 112]) {
      ctx.fillStyle = "#1b1a2a";
      ctx.fillRect(lx, y + 11, 10, 5);
      ctx.fillStyle = "#ff2bd6";
      ctx.fillRect(lx, y + 10, 10, 1);
    }
    const cx = x + w / 2;
    const pulse = 0.25 + 0.15 * Math.sin(now / 300);
    ctx.fillStyle = `rgba(0,229,255,${pulse.toFixed(2)})`;
    ctx.fillRect(cx - 5, y + 8, 10, 8);
    ctx.fillStyle = "#00e5ff";
    ctx.fillRect(cx - 3, y + 15, 6, 1);
    ctx.fillRect(cx - 0.25, y + 9, 0.5, 6);
  },
  sofa(ctx, f) {
    const x = f.x * TILE;
    const y = f.y * TILE;
    const w = f.w * TILE;
    // Magenta leather with a cyan underglow.
    ctx.fillStyle = "#00e5ff";
    ctx.fillRect(x + 1, y + 12, w - 2, 0.5);
    ctx.fillStyle = "#4a1540";
    ctx.fillRect(x, y - 2, w, 7);
    ctx.fillStyle = "#6b1f5a";
    ctx.fillRect(x, y + 5, w, 7);
    ctx.fillStyle = "#521746";
    for (let i = 16; i < w; i += 16) ctx.fillRect(x + i, y + 5, 0.5, 7);
    ctx.fillStyle = "#8a2a74";
    ctx.fillRect(x + 3, y + 5, w - 6, 0.5);
    ctx.fillStyle = "#3d1036";
    ctx.fillRect(x, y - 2, 3, 14);
    ctx.fillRect(x + w - 3, y - 2, 3, 14);
  },
  coffee(ctx, f, now) {
    const x = f.x * TILE;
    const y = f.y * TILE;
    // Vending machine with a glowing front.
    ctx.fillStyle = "#15142a";
    ctx.fillRect(x + 2, y - 8, 12, 23);
    ctx.fillStyle = Math.floor(now / 1700) % 9 === 0 ? "#a01d8a" : "#ff2bd6";
    ctx.fillRect(x + 3, y - 7, 7, 13);
    ctx.fillStyle = "#2a0b26";
    for (let j = 0; j < 4; j++) ctx.fillRect(x + 3.5, y - 5.5 + j * 3, 6, 1);
    ctx.fillStyle = "#00e5ff";
    for (let j = 0; j < 3; j++) ctx.fillRect(x + 11.5, y - 5 + j * 2, 1, 1);
    ctx.fillStyle = "#07060c";
    ctx.fillRect(x + 4, y + 9, 6, 2);
  },
  plant(ctx, f) {
    const x = f.x * TILE;
    const y = f.y * TILE;
    // Bioluminescent plant in a black pot with a cyan ring.
    ctx.fillStyle = "#0b0a12";
    ctx.fillRect(x + 4, y + 8, 8, 7);
    ctx.fillStyle = "#00e5ff";
    ctx.fillRect(x + 4, y + 9, 8, 0.5);
    ctx.fillStyle = "#0f5d55";
    ctx.fillRect(x + 1, y - 3, 14, 11);
    ctx.fillStyle = "#1fd1b4";
    ctx.fillRect(x + 3, y - 6, 5, 5);
    ctx.fillRect(x + 9, y - 4, 5, 5);
    ctx.fillRect(x + 2, y + 2, 5, 4);
    ctx.fillStyle = "#7dfce8";
    for (const [dx, dy] of [[4, -5.5], [11, -3.5], [3, 2.5], [9, 4], [6, 0]]) ctx.fillRect(x + dx, y + dy, 0.5, 0.5);
  },
  bookshelf(ctx, f) {
    const x = f.x * TILE;
    const y = f.y * TILE;
    const w = f.w * TILE;
    // Data shelf: black frame, glowing storage cubes.
    ctx.fillStyle = "#07060c";
    ctx.fillRect(x, y - 10, w, 25);
    const glow = ["#ff2bd6", "#00e5ff", "#7c4dff"];
    for (let row = 0; row < 3; row++) {
      ctx.fillStyle = "#24203a";
      ctx.fillRect(x + 1, y - 3 + row * 8, w - 2, 0.5);
      for (let i = 0; i < 7; i++) {
        if ((i + row * 2) % 4 === 3) continue;
        ctx.fillStyle = glow[(i + row) % glow.length];
        ctx.fillRect(x + 2 + i * 4, y - 6 + row * 8, 2.5, 2.5);
      }
    }
  },
  whiteboard(ctx, f) {
    const x = f.x * TILE;
    const y = f.y * TILE;
    const w = f.w * TILE;
    // Holo board: dark panel, cyan frame, a magenta chart.
    ctx.fillStyle = "#00e5ff";
    ctx.fillRect(x, y - 12, w, 12);
    ctx.fillStyle = "#0b0a18";
    ctx.fillRect(x + 0.5, y - 11.5, w - 1, 11);
    ctx.fillStyle = "rgba(0,229,255,0.7)";
    for (let i = 0; i < Math.floor((w - 8) / 4); i++) ctx.fillRect(x + 4 + i * 4, y - 3 - ((i * 7 + f.x) % 6), 2, 2 + ((i * 7 + f.x) % 6));
    ctx.fillStyle = "#ff2bd6";
    for (let i = 0; i < w - 8; i += 1) ctx.fillRect(x + 4 + i, y - 9 + Math.round(Math.sin((i + f.x) / 3) * 1.5), 1, 0.5);
  },
  waterCooler(ctx, f) {
    const x = f.x * TILE;
    const y = f.y * TILE;
    ctx.fillStyle = "#1b1a2a";
    ctx.fillRect(x + 4, y + 2, 8, 13);
    ctx.fillStyle = "#ff2bd6";
    ctx.fillRect(x + 4, y + 8, 8, 0.5);
    ctx.fillStyle = "rgba(0,229,255,0.75)";
    ctx.fillRect(x + 5, y - 6, 6, 8);
    ctx.fillStyle = "#c9fbff";
    ctx.fillRect(x + 6, y - 5, 0.5, 5);
  },
};


function noirDecor(ctx: CanvasRenderingContext2D, kind: string, x: number, y: number) {
  if (kind === "clock") {
    // Brass-rimmed wall clock.
    ctx.fillStyle = "#b08a3e";
    ctx.fillRect(x + 3.5, y + 3.5, 9, 9);
    ctx.fillStyle = "#e8dcc0";
    ctx.fillRect(x + 4, y + 4, 8, 8);
    ctx.fillStyle = "#1a1410";
    ctx.fillRect(x + 7.75, y + 5, 0.5, 3.5);
    ctx.fillRect(x + 8, y + 8, 2.5, 0.5);
  } else {
    // A WANTED bill: yellowed paper, a silhouette, a line of type.
    ctx.fillStyle = "#d8c8a0";
    ctx.fillRect(x + 3, y + 2, 10, 12);
    ctx.fillStyle = "#3a2e22";
    ctx.fillRect(x + 4, y + 3, 8, 1);
    ctx.fillRect(x + 6.5, y + 5, 3, 2.5);
    ctx.fillRect(x + 5.5, y + 7.5, 5, 3);
    ctx.fillRect(x + 4.5, y + 11.5, 7, 0.5);
    ctx.fillRect(x + 5.5, y + 12.5, 5, 0.5);
  }
}

/** L.A. Noire pieces: same footprints, 1947 objects. */
const NOIR_FURNITURE: WorldTheme["furniture"] = {
  ceoDesk(ctx, f) {
    const x = f.x * TILE;
    const y = f.y * TILE;
    const w = f.w * TILE;
    // Mahogany desk with a green leather inlay, a banker's lamp and a rotary phone.
    ctx.fillStyle = "#2a160e";
    ctx.fillRect(x, y + 1, w, 10);
    ctx.fillStyle = "#3a1f14";
    ctx.fillRect(x, y + 1, w, 8);
    ctx.fillStyle = "#2f4a36";
    ctx.fillRect(x + 8, y + 2.5, w - 16, 5);
    ctx.fillStyle = "#c9a227";
    ctx.fillRect(x + 34, y - 1, 1, 4);
    ctx.fillStyle = "#2f6a42";
    ctx.fillRect(x + 31, y - 3, 7, 2.5);
    ctx.fillStyle = "#f2d38a";
    ctx.fillRect(x + 32, y - 0.5, 5, 0.5);
    ctx.fillStyle = "#111";
    ctx.fillRect(x + 13, y + 3, 6, 4);
    ctx.fillRect(x + 12, y + 2, 8, 1.5);
    ctx.fillStyle = "#c9a227";
    ctx.fillRect(x + 15.5, y + 4.5, 1, 1);
    ctx.fillStyle = "#e8e0cc";
    ctx.fillRect(x + 22, y + 3, 6, 4);
  },
  meetingTable(ctx, f, now) {
    const x = f.x * TILE;
    const y = f.y * TILE;
    const w = f.w * TILE;
    const h = f.h * TILE;
    // Interrogation: a bare steel table under one hanging bulb, a case file, an ashtray.
    ctx.fillStyle = "#4a4a46";
    ctx.fillRect(x + 4, y + 3, w - 8, h - 8);
    ctx.fillStyle = "#5e5e58";
    ctx.fillRect(x + 4, y + 3, w - 8, 0.5);
    ctx.fillStyle = "#353532";
    ctx.fillRect(x + 4, y + h - 5.5, w - 8, 0.5);
    ctx.fillStyle = "#c9a86a";
    ctx.fillRect(x + 22, y + 9, 12, 8);
    ctx.fillStyle = "#e8dcc0";
    ctx.fillRect(x + 23, y + 10, 9, 0.5);
    ctx.fillRect(x + 23, y + 11.5, 7, 0.5);
    ctx.fillStyle = "#2a2a28";
    ctx.fillRect(x + 108, y + 11, 5, 4);
    ctx.fillStyle = "#6b6b66";
    ctx.fillRect(x + 109, y + 12, 3, 2);
    const cx = x + w / 2;
    // The bulb's pool of light, in three stepped rings; it stutters now and then.
    const flick = Math.floor(now / 2300) % 11 === 0 ? 0.5 : 1;
    ctx.fillStyle = `rgba(255,214,150,${(0.09 * flick).toFixed(3)})`;
    for (const [rx, ry] of [
      [26, 11],
      [17, 7.5],
      [9, 4],
    ]) {
      ctx.beginPath();
      ctx.ellipse(cx, y + 12, rx, ry, 0, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.fillStyle = "#1a1612";
    ctx.fillRect(cx - 0.25, y - 6, 0.5, 9);
    ctx.fillStyle = "#fff2c8";
    ctx.fillRect(cx - 1, y + 3, 2, 2);
  },
  serverRack(ctx, f) {
    const x = f.x * TILE;
    const y = f.y * TILE;
    // Olive filing cabinet with brass handles and paper labels.
    ctx.fillStyle = "#3e3f36";
    ctx.fillRect(x + 1, y - 10, 14, 26);
    for (let i = 0; i < 4; i++) {
      ctx.fillStyle = "#4c4d42";
      ctx.fillRect(x + 2, y - 9 + i * 6.25, 12, 5.5);
      ctx.fillStyle = "#e8dcc0";
      ctx.fillRect(x + 6, y - 8 + i * 6.25, 4, 1.5);
      ctx.fillStyle = "#c9a227";
      ctx.fillRect(x + 6.5, y - 5.5 + i * 6.25, 3, 0.5);
    }
  },
  sofa(ctx, f) {
    const x = f.x * TILE;
    const y = f.y * TILE;
    const w = f.w * TILE;
    // Worn chesterfield with button tufting.
    ctx.fillStyle = "#4a2618";
    ctx.fillRect(x, y - 2, w, 7);
    ctx.fillStyle = "#5e3020";
    ctx.fillRect(x, y + 5, w, 7);
    ctx.fillStyle = "#2e170e";
    for (let i = 4; i < w - 2; i += 4) ctx.fillRect(x + i, y + (i % 8 ? 0 : 2), 0.5, 0.5);
    ctx.fillStyle = "#7a4a30";
    ctx.fillRect(x + 3, y + 5, w - 6, 0.5);
    ctx.fillStyle = "#3a1c11";
    ctx.fillRect(x, y - 2, 3, 14);
    ctx.fillRect(x + w - 3, y - 2, 3, 14);
  },
  coffee(ctx, f) {
    const x = f.x * TILE;
    const y = f.y * TILE;
    // A chrome percolator on a wooden counter, a cup beside it.
    ctx.fillStyle = "#3a2a1c";
    ctx.fillRect(x + 1, y + 2, 14, 13);
    ctx.fillStyle = "#4a3624";
    ctx.fillRect(x + 1, y + 2, 14, 0.5);
    ctx.fillStyle = "#b8b4aa";
    ctx.fillRect(x + 5, y - 5, 6, 8);
    ctx.fillStyle = "#e0ddd4";
    ctx.fillRect(x + 5.5, y - 4.5, 1, 6);
    ctx.fillStyle = "#3a3632";
    ctx.fillRect(x + 7, y - 7, 2, 2);
    ctx.fillStyle = "#e8e0cc";
    ctx.fillRect(x + 11.5, y + 3, 2, 2);
  },
  plant(ctx, f) {
    const x = f.x * TILE;
    const y = f.y * TILE;
    // A potted palm in terracotta.
    ctx.fillStyle = "#8a4a2a";
    ctx.fillRect(x + 4, y + 8, 8, 7);
    ctx.fillStyle = "#6e3a20";
    ctx.fillRect(x + 10, y + 8, 2, 7);
    ctx.fillStyle = "#5a4630";
    ctx.fillRect(x + 7.5, y + 1, 1, 7);
    ctx.fillStyle = "#2f4a2a";
    for (const [dx, dy, len, dir] of [[8, 1, 7, -1], [8, 1, 7, 1], [8, 0, 5, -0.4], [8, 0, 5, 0.4], [8, 2, 6, -1.6], [8, 2, 6, 1.6]] as const)
      for (let i = 0; i < len; i += 0.5) ctx.fillRect(x + dx + i * dir * 0.8, y + dy - i * 0.5 + (i * i) / 14, 1, 0.5);
    ctx.fillStyle = "#4a6a3a";
    ctx.fillRect(x + 5, y - 2, 0.5, 0.5);
    ctx.fillRect(x + 11, y - 2, 0.5, 0.5);
  },
  bookshelf(ctx, f) {
    const x = f.x * TILE;
    const y = f.y * TILE;
    const w = f.w * TILE;
    // Law books in dark wood.
    ctx.fillStyle = "#2a1a10";
    ctx.fillRect(x, y - 10, w, 25);
    const colors = ["#5a1f1f", "#1f2a4a", "#3a4a2a", "#8a6a42", "#4a3322"];
    for (let row = 0; row < 3; row++) {
      ctx.fillStyle = "#4a3322";
      ctx.fillRect(x + 1, y - 3 + row * 8, w - 2, 0.5);
      for (let i = 0; i < 7; i++) {
        ctx.fillStyle = colors[(i + row * 2) % colors.length];
        ctx.fillRect(x + 2 + i * 4, y - 8 + row * 8, 3, 5);
        ctx.fillStyle = "#c9a227";
        ctx.fillRect(x + 2.5 + i * 4, y - 7 + row * 8, 2, 0.5);
      }
    }
  },
  whiteboard(ctx, f) {
    const x = f.x * TILE;
    const y = f.y * TILE;
    const w = f.w * TILE;
    // Evidence board: cork, pinned photos and red string between them.
    ctx.fillStyle = "#4a3322";
    ctx.fillRect(x, y - 12, w, 12);
    ctx.fillStyle = "#8a6a42";
    ctx.fillRect(x + 1, y - 11, w - 2, 10);
    const pins: [number, number][] = [];
    for (let i = 0; i < Math.floor((w - 6) / 7); i++) {
      const px = x + 3 + i * 7;
      const py = y - 10 + ((i * 5 + f.x) % 4);
      ctx.fillStyle = (i + f.x) % 3 ? "#e8e0cc" : "#d8c8a0";
      ctx.fillRect(px, py, 4, 4);
      ctx.fillStyle = "#3a3632";
      ctx.fillRect(px + 1, py + 1, 2, 2);
      pins.push([px + 2, py]);
    }
    ctx.strokeStyle = "#c8323c";
    ctx.lineWidth = 0.5;
    ctx.beginPath();
    pins.forEach(([px, py], i) => (i ? ctx.lineTo(px, py) : ctx.moveTo(px, py)));
    ctx.stroke();
    ctx.fillStyle = "#c8323c";
    for (const [px, py] of pins) ctx.fillRect(px - 0.25, py - 0.25, 1, 1);
  },
  waterCooler(ctx, f) {
    const x = f.x * TILE;
    const y = f.y * TILE;
    // Glass jug on a wooden stand.
    ctx.fillStyle = "#3a2a1c";
    ctx.fillRect(x + 4, y + 2, 8, 13);
    ctx.fillStyle = "rgba(200,220,230,0.55)";
    ctx.fillRect(x + 5, y - 6, 6, 8);
    ctx.fillStyle = "#f0f4f2";
    ctx.fillRect(x + 6, y - 5, 0.5, 5);
  },
};

const MODERN: WorldTheme = {
  backdrop: "#0f1117",
  void: "#151821",
  floors: MODERN_FLOORS,
  night: "6,8,22",
  nightAlpha: 0.48,
  lamps: ["255,190,110"],
  lampAlpha: 0.06,
  chair: { frame: "#1c1f28", meshA: "#2c3240", meshB: "#262b37", trim: "#4a5266" },
  glass: { top: "#c9d0dc", pane: "rgba(150,195,240,0.38)", bottom: "#6f788b", shine: "rgba(255,255,255,0.55)", post: "#8a93a6" },
  wall: { body: "#3a4052", cap: "#566078", edge: "#6b7590", base: "#2c3140" },
  sky: { top: "#101831", low: "#17213f", star: "#cfd6e6", tower: "#0b1022", lights: ["#f2c94c", "#e9a86a"], head: "#252a38", mullion: "#3a4052", sill: "#6b7590" },
  desk: {
    top: "#e9ecf1",
    edge: "#c3c9d4",
    shine: "#ffffff",
    legs: "#2b2f3a",
    bezel: "#1b1d24",
    stand: "#8a93a6",
    kbd: "#d6dae2",
    keys: ["#b9bfca"],
    keyRow: "#b9bfca",
    mug: "#f2efe8",
  },
  console: { top: "#4a5060", edge: "#363b48", shine: "#5a6072" },
  rack: { body: "#15171d", panel: "#22252e", leds: ["#4c8df5", "#3bd1c6"], off: "#2a2e38" },
  furniture: {},
  decor: modernDecor,
  neon: { halo: "#ff6b9a", core: "#ffe3ec", glow: "255,107,154" },
  plate: { shadow: "#3d2a1c", body: "#8a5a33", bar: "#f2c94c", text: "#fff6e6" },
  mat: "#6b4a3a",
  weather: false,
  daylight: true,
};

const CYBERPUNK: WorldTheme = {
  backdrop: "#07060f",
  void: "#07060f",
  floors: CYBER_FLOORS,
  night: "8,4,24",
  nightAlpha: 0.36,
  lamps: ["255,43,214", "0,229,255", "124,77,255"],
  lampAlpha: 0.16,
  chair: { frame: "#0b0a12", meshA: "#1a1626", meshB: "#141120", trim: "#ff2bd6" },
  glass: { top: "#ff2bd6", pane: "rgba(0,229,255,0.2)", bottom: "#00e5ff", shine: "rgba(255,255,255,0.6)", post: "#ff2bd6" },
  wall: { body: "#0e0d1a", cap: "#1d1b33", edge: "#2c2950", base: "#08070f", trim: "#00e5ff" },
  sky: { top: "#12062a", low: "#2a0b3d", star: "#ff9be8", tower: "#07040f", lights: ["#ff2bd6", "#00e5ff", "#ffd166", "#7c4dff"], head: "#0e0d1a", mullion: "#2c2950", sill: "#ff2bd6" },
  desk: {
    top: "#14131f",
    edge: "#0b0a12",
    shine: "#2a2840",
    legs: "#3a3756",
    bezel: "#07060c",
    stand: "#3a3756",
    kbd: "#1b1a2a",
    keys: ["#ff2bd6", "#00e5ff", "#ffd166"],
    keyRow: "#2a2840",
    mug: "#00e5ff",
    trim: "#ff2bd6",
  },
  console: { top: "#101a20", edge: "#08090d", shine: "#1d2a33", trim: "#00e5ff" },
  benchTrim: "#00ff9f",
  rack: { body: "#0b0a12", panel: "#17142a", leds: ["#ff2bd6", "#00e5ff"], off: "#1d1a2e" },
  furniture: CYBER_FURNITURE,
  decor: cyberDecor,
  neon: { halo: "#00e5ff", core: "#e6fdff", glow: "0,229,255" },
  plate: { shadow: "#ff2bd6", body: "#140f24", bar: "#00e5ff", text: "#f5e9ff" },
  mat: "#3a1146",
  weather: true,
};


const NOIRE: WorldTheme = {
  backdrop: "#0d0c0b",
  void: "#0b0a09",
  floors: NOIR_FLOORS,
  night: "10,8,6",
  nightAlpha: 0.52,
  lamps: ["255,200,120"],
  lampAlpha: 0.12,
  chair: { frame: "#2a1d14", meshA: "#4a3322", meshB: "#43301f", trim: "#7a5a3a" },
  glass: { top: "#6b4a2e", pane: "rgba(220,215,200,0.32)", bottom: "#3e2a1a", shine: "rgba(255,250,235,0.45)", post: "#5a3d25" },
  wall: { body: "#2b2620", cap: "#4a4036", edge: "#6b5d4d", base: "#1a1612" },
  sky: { top: "#1a1c22", low: "#2a2620", star: "#3a3c44", tower: "#121317", lights: ["#f2c26b", "#e8a050"], head: "#2b2620", mullion: "#4a4036", sill: "#6b5d4d" },
  desk: {
    top: "#4a3322",
    edge: "#2e1f14",
    shine: "#6b4a30",
    legs: "#2a1d14",
    bezel: "#1a1410",
    stand: "#5a4a3a",
    kbd: "#2a2622",
    keys: ["#d8d0bf"],
    keyRow: "#3a3530",
    mug: "#e8e0cc",
  },
  console: { top: "#3e3f36", edge: "#2a2b24", shine: "#55574b" },
  rack: { body: "#3e3f36", panel: "#4c4d42", leds: ["#c9a227", "#c9a227"], off: "#2a2b24" },
  furniture: NOIR_FURNITURE,
  decor: noirDecor,
  neon: { halo: "#ff5a36", core: "#ffe0c8", glow: "255,90,54" },
  plate: { shadow: "#1a1410", body: "#9c7a3c", bar: "#e0c070", text: "#1c140c" },
  mat: "#3a2a1c",
  weather: true,
  blinds: true,
  vignette: true,
};

export const THEMES: Record<OfficeTheme, WorldTheme> = { modern: MODERN, cyberpunk: CYBERPUNK, noire: NOIRE };

// ponytail: module-level active palette (one office renderer per window); pass it through the painters if two ever coexist.
let T: WorldTheme = MODERN;

function drawStatic(layout: OfficeLayout, scene: OfficeScene): HTMLCanvasElement | null {
  if (typeof document === "undefined") return null;
  const canvas = document.createElement("canvas");
  canvas.width = layout.width * TILE * ART;
  canvas.height = layout.height * TILE * ART;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  // Drawn in layout pixels at ART density, so details can be half a pixel.
  ctx.scale(ART, ART);
  ctx.fillStyle = T.void;
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  const { grid } = scene;
  const cellAt = (tx: number, ty: number) => (tx < 0 || ty < 0 || tx >= grid.width || ty >= grid.height ? "void" : grid.cells[ty * grid.width + tx]);
  const floorOf = new Map(layout.rooms.map((r) => [r.id, r.floor]));
  // Walls on the building's edge are solid; walls between spaces are glass.
  const exterior = (tx: number, ty: number) =>
    cellAt(tx, ty) === "wall" &&
    [-1, 0, 1].some((dx) => [-1, 0, 1].some((dy) => cellAt(tx + dx, ty + dy) === "void"));
  for (let ty = 0; ty < grid.height; ty++) {
    for (let tx = 0; tx < grid.width; tx++) {
      const i = ty * grid.width + tx;
      const cell = grid.cells[i];
      const x = tx * TILE;
      const y = ty * TILE;
      if (cell === "corridor") T.floors.corridor(ctx, x, y);
      else if (cell === "floor" || cell === "door" || (cell === "wall" && !exterior(tx, ty))) {
        const floor = floorOf.get(grid.roomAt[i] ?? "") ?? "carpet";
        (T.floors[floor] ?? T.floors.carpet)(ctx, x, y);
        if (cell === "door") {
          ctx.fillStyle = "rgba(0,0,0,0.12)";
          ctx.fillRect(x, y, TILE, TILE);
        }
      }
      // The wall above casts a short shadow onto the floor.
      if (cell !== "wall" && cell !== "void" && exterior(tx, ty - 1)) {
        ctx.fillStyle = "rgba(0,0,0,0.28)";
        ctx.fillRect(x, y, TILE, 2);
        ctx.fillStyle = "rgba(0,0,0,0.14)";
        ctx.fillRect(x, y + 2, TILE, 2);
      }
    }
  }
  // Seen from outside at night: floors sit in the dark, lit by ceiling lamps.
  drawNight(ctx, layout, grid, layout.width * TILE, layout.height * TILE);
  if (T.blinds) drawBlinds(ctx, layout, grid, cellAt);
  drawChairs(ctx, layout);
  for (let ty = 0; ty < grid.height; ty++) {
    for (let tx = 0; tx < grid.width; tx++) {
      if (grid.cells[ty * grid.width + tx] !== "wall") continue;
      const x = tx * TILE;
      const y = ty * TILE;
      if (!exterior(tx, ty)) {
        drawGlass(ctx, tx, ty, cellAt);
        continue;
      }
      ctx.fillStyle = T.wall.body;
      ctx.fillRect(x, y, TILE, TILE);
      ctx.fillStyle = T.wall.cap;
      ctx.fillRect(x, y, TILE, 4);
      ctx.fillStyle = T.wall.edge;
      ctx.fillRect(x, y, TILE, 1);
      ctx.fillStyle = T.wall.base;
      ctx.fillRect(x, y + TILE - 2, TILE, 2);
      // The neon trim runs along the foot of a wall, not across every tile of a vertical one.
      if (T.wall.trim && cellAt(tx, ty + 1) !== "wall") {
        ctx.fillStyle = T.wall.trim;
        ctx.fillRect(x, y + TILE - 2, TILE, 0.5);
      }
    }
  }
  drawWindows(ctx, layout, cellAt);
  for (const d of layout.decorations) {
    const x = d.x * TILE;
    const y = d.y * TILE;
    if (d.kind === "clock" || d.kind === "poster") {
      T.decor(ctx, d.kind, x, y);
    } else if (d.kind === "neon") {
      // "</>" in pink neon, with its glow on the glass.
      const cx = x + TILE / 2;
      const cy = y + TILE / 2;
      ctx.globalCompositeOperation = "lighter";
      const g = ctx.createRadialGradient(cx, cy, 0, cx, cy, 22);
      g.addColorStop(0, `rgba(${T.neon.glow},0.32)`);
      g.addColorStop(1, `rgba(${T.neon.glow},0)`);
      ctx.fillStyle = g;
      ctx.fillRect(cx - 22, cy - 22, 44, 44);
      ctx.globalCompositeOperation = "source-over";
      const glyph = ["..x.....x.x..", ".x.....x...x.", "x.....x.....x", ".x...x.....x.", "..x.x.....x.."];
      // Tube halo first (one pixel around each lit pixel), then the hot core.
      for (const [color, pad] of [
        [T.neon.halo, 1],
        [T.neon.core, 0],
      ] as const) {
        ctx.fillStyle = color;
        glyph.forEach((row, j) => {
          for (let i = 0; i < row.length; i++) if (row[i] === "x") ctx.fillRect(x + 1 + i - pad, y + 6 + j - pad, 1 + pad * 2, 1 + pad * 2);
        });
      }
    }
  }
  // Entrance mat.
  ctx.fillStyle = T.mat;
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
      for (let row = 0; row < h - 1; row += 1) {
        const n = hashString(`${seed}:${scroll + row}`);
        if (activity === "THINKING" && row > 2) break;
        ctx.fillStyle = colors[n % colors.length];
        ctx.fillRect(x + 0.5 + (n % 3) / 2, y + 0.5 + row, 1 + ((n >>> 4) % (w - 2)), 0.5);
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
      for (let row = 1; row < h - 0.5; row += 1) {
        const n = hashString(`${seed}:${scroll + row}`);
        ctx.fillRect(x + 0.5, y + row, 2 + (n % (w - 3)), 0.5);
      }
      return;
    }
    case "RUNNING_COMMAND":
    case "TESTING": {
      ctx.fillStyle = "#0c1410";
      ctx.fillRect(x, y, w, h);
      ctx.fillStyle = activity === "TESTING" ? "#7bdc6b" : "#3fdc7a";
      for (let row = 0; row < h - 1; row += 1) {
        const n = hashString(`${seed}:${scroll + row}`);
        ctx.fillRect(x + 0.5, y + 0.5 + row, 1 + (n % (w - 2)), 0.5);
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
  // Contact shadow so pieces stand on the floor (wall-mounted boards excepted).
  if (f.solid !== false) {
    ctx.fillStyle = "rgba(0,0,0,0.24)";
    ctx.fillRect(x + 1, y + f.h * TILE - 2, w - 1, 3);
  }
  const custom = T.furniture[f.kind];
  if (custom) return custom(ctx, f, now);
  switch (f.kind) {
    case "desk":
    case "console":
    case "testBench": {
      // Desks on thin legs (the terminal room's consoles in their own finish).
      const D = T.desk;
      const finish = f.kind === "console" ? T.console : D;
      const trim = f.kind === "console" ? T.console.trim : f.kind === "testBench" ? T.benchTrim : D.trim;
      ctx.fillStyle = D.legs;
      ctx.fillRect(x + 2, y + 10, 0.5, 6);
      ctx.fillRect(x + w - 2.5, y + 10, 0.5, 6);
      ctx.fillStyle = finish.top;
      ctx.fillRect(x, y + 1, w, 8);
      ctx.fillStyle = finish.shine;
      ctx.fillRect(x, y + 1, w, 0.5);
      ctx.fillStyle = finish.edge;
      ctx.fillRect(x, y + 9, w, 2);
      if (trim) {
        // Neon strip along the front edge.
        ctx.fillStyle = trim;
        ctx.fillRect(x, y + 10.5, w, 0.5);
      }
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
      // Two monitors on stands either side of the person, keyboard in front.
      const screen = f.kind === "console" && activity ? "RUNNING_COMMAND" : activity;
      for (const [mx, s] of [
        [x + 1, seed],
        [x + w - 10, seed + 3],
      ]) {
        ctx.fillStyle = D.bezel;
        ctx.fillRect(mx, y - 7, 9, 7.5);
        ctx.fillStyle = D.stand;
        ctx.fillRect(mx + 4.25, y + 0.5, 0.5, 2.5);
        ctx.fillRect(mx + 2.5, y + 3, 4, 0.5);
        drawScreen(ctx, mx + 0.5, y - 6.5, 8, 6.5, screen, now, s);
      }
      ctx.fillStyle = D.kbd;
      ctx.fillRect(x + 12, y + 4, 9, 2);
      // Key rows.
      for (let k = 0; k < 9; k += 1) {
        ctx.fillStyle = D.keys[k % D.keys.length];
        ctx.fillRect(x + 12.25 + k, y + 4.5, 0.5, 0.5);
      }
      ctx.fillStyle = D.keyRow;
      ctx.fillRect(x + 12, y + 5.5, 9, 0.5);
      if (seed % 2) {
        ctx.fillStyle = D.mug;
        ctx.fillRect(x + w - 3, y + 5, 2, 2);
      }
      break;
    }
    case "ceoDesk": {
      // Light oak slab on black legs, an all-in-one screen and a laptop.
      ctx.fillStyle = "#1b1d24";
      ctx.fillRect(x + 2, y + 10, 1, 5);
      ctx.fillRect(x + w - 3, y + 10, 1, 5);
      ctx.fillStyle = "#d8c6a6";
      ctx.fillRect(x, y + 1, w, 8);
      ctx.fillStyle = "#e6d8be";
      ctx.fillRect(x, y + 1, w, 1);
      ctx.fillStyle = "#b5a283";
      ctx.fillRect(x, y + 9, w, 2);
      ctx.fillStyle = "#c9ced8";
      ctx.fillRect(x + 30, y - 7, 14, 9);
      ctx.fillStyle = "#2f4f8f";
      ctx.fillRect(x + 31, y - 6, 12, 6);
      ctx.fillStyle = "#4c76c2";
      ctx.fillRect(x + 31, y - 6, 12, 2);
      ctx.fillStyle = "#aeb6c4";
      ctx.fillRect(x + 36, y + 2, 2, 2);
      ctx.fillRect(x + 14, y + 3, 10, 5);
      ctx.fillStyle = "#2b2f3a";
      ctx.fillRect(x + 14, y + 2, 10, 1);
      break;
    }
    case "meetingTable": {
      // Long white table with rounded ends, two laptops and a speaker puck.
      const h = f.h * TILE;
      ctx.fillStyle = "#c3c9d4";
      ctx.fillRect(x + 4, y + 3, w - 8, h - 8);
      ctx.fillRect(x + 2, y + 5, w - 4, h - 12);
      ctx.fillStyle = "#e9ecf1";
      ctx.fillRect(x + 4, y + 3, w - 8, h - 10);
      ctx.fillRect(x + 2, y + 5, w - 4, h - 14);
      ctx.fillStyle = "#f6f7fa";
      ctx.fillRect(x + 4, y + 3, w - 8, 1);
      for (const lx of [x + 20, x + 112]) {
        ctx.fillStyle = "#aeb6c4";
        ctx.fillRect(lx, y + 11, 10, 5);
        ctx.fillStyle = "#2b2f3a";
        ctx.fillRect(lx, y + 10, 10, 1);
      }
      ctx.fillStyle = "#2b2f3a";
      ctx.fillRect(x + w / 2 - 3, y + 12, 6, 4);
      ctx.fillStyle = "#4c8df5";
      ctx.fillRect(x + w / 2 - 1, y + 13, 2, 1);
      break;
    }
    case "serverRack": {
      ctx.fillStyle = T.rack.body;
      ctx.fillRect(x + 1, y - 10, 14, 26);
      ctx.fillStyle = T.rack.panel;
      for (let i = 0; i < 5; i++) ctx.fillRect(x + 3, y - 8 + i * 5, 10, 3);
      for (let i = 0; i < 5; i++) {
        const on = (Math.floor(now / 300) + i + f.x) % 3 !== 0;
        ctx.fillStyle = on ? T.rack.leds[i % 2] : T.rack.off;
        ctx.fillRect(x + 11, y - 7 + i * 5, 1, 1);
      }
      break;
    }
    case "sofa": {
      // Modular sofa in sage: back, seat cushions, arms.
      ctx.fillStyle = "#5f7a6c";
      ctx.fillRect(x, y - 2, w, 7);
      ctx.fillStyle = "#7a9686";
      ctx.fillRect(x, y + 5, w, 7);
      ctx.fillStyle = "#6a8576";
      for (let i = 16; i < w; i += 16) ctx.fillRect(x + i, y + 5, 1, 7);
      ctx.fillStyle = "#8aa797";
      ctx.fillRect(x + 3, y + 5, w - 6, 1);
      ctx.fillStyle = "#536b5e";
      ctx.fillRect(x, y - 2, 3, 14);
      ctx.fillRect(x + w - 3, y - 2, 3, 14);
      ctx.fillStyle = "#2b2f3a";
      ctx.fillRect(x + 1, y + 12, 1, 2);
      ctx.fillRect(x + w - 2, y + 12, 1, 2);
      break;
    }
    case "coffee": {
      // Espresso machine on a white counter.
      ctx.fillStyle = "#e9ecf1";
      ctx.fillRect(x + 1, y + 2, 14, 13);
      ctx.fillStyle = "#c3c9d4";
      ctx.fillRect(x + 1, y + 13, 14, 2);
      ctx.fillStyle = "#aeb6c4";
      ctx.fillRect(x + 3, y - 7, 10, 9);
      ctx.fillStyle = "#d6dae2";
      ctx.fillRect(x + 3, y - 7, 10, 1);
      ctx.fillStyle = "#2b2f3a";
      ctx.fillRect(x + 5, y - 3, 6, 3);
      ctx.fillStyle = "#ff5c5c";
      ctx.fillRect(x + 11, y - 6, 1, 1);
      ctx.fillStyle = "#f2efe8";
      ctx.fillRect(x + 7, y + 3, 2, 2);
      break;
    }
    case "plant": {
      // A big leafy plant in a white pot.
      ctx.fillStyle = "#e9ecf1";
      ctx.fillRect(x + 4, y + 8, 8, 7);
      ctx.fillStyle = "#c3c9d4";
      ctx.fillRect(x + 10, y + 8, 2, 7);
      ctx.fillStyle = "#2f7a46";
      ctx.fillRect(x + 1, y - 3, 14, 11);
      ctx.fillStyle = "#3fa35c";
      ctx.fillRect(x + 3, y - 6, 5, 5);
      ctx.fillRect(x + 9, y - 4, 5, 5);
      ctx.fillRect(x + 2, y + 2, 5, 4);
      ctx.fillStyle = "#5fc07a";
      ctx.fillRect(x + 4, y - 5, 1, 2);
      ctx.fillRect(x + 11, y - 3, 1, 2);
      break;
    }
    case "bookshelf": {
      // Open shelving: black frame, oak shelves, books and a few objects.
      ctx.fillStyle = "#1b1d24";
      ctx.fillRect(x, y - 10, w, 25);
      const colors = ["#c9ced8", "#5f7a6c", "#d97757", "#e9ecf1", "#4c8df5"];
      for (let row = 0; row < 3; row++) {
        ctx.fillStyle = "#c49a6c";
        ctx.fillRect(x + 1, y - 3 + row * 8, w - 2, 1);
        for (let i = 0; i < 7; i++) {
          if ((i + row * 2) % 4 === 3) continue;
          ctx.fillStyle = colors[(i + row) % colors.length];
          ctx.fillRect(x + 2 + i * 4, y - 8 + row * 8, 3, 5);
        }
      }
      break;
    }
    case "whiteboard": {
      // Glass board on the wall with sticky notes.
      ctx.fillStyle = "#c9ced8";
      ctx.fillRect(x, y - 12, w, 12);
      ctx.fillStyle = "#dfe7ef";
      ctx.fillRect(x + 1, y - 11, w - 2, 10);
      ctx.fillStyle = "rgba(255,255,255,0.6)";
      ctx.fillRect(x + 2, y - 11, 3, 1);
      const notes = ["#f2d36b", "#ff9fbf", "#8ab4ff", "#8fdca0"];
      for (let i = 0; i < Math.floor((w - 8) / 5); i++) {
        if ((i + f.x) % 3 === 2) continue;
        ctx.fillStyle = notes[(i + f.x) % notes.length];
        ctx.fillRect(x + 4 + i * 5, y - 9 + (i % 2) * 4, 3, 3);
      }
      break;
    }
    case "waterCooler": {
      ctx.fillStyle = "#e9ecf1";
      ctx.fillRect(x + 4, y + 2, 8, 13);
      ctx.fillStyle = "#c3c9d4";
      ctx.fillRect(x + 10, y + 2, 2, 13);
      ctx.fillStyle = "#8ab4ff";
      ctx.fillRect(x + 5, y - 6, 6, 8);
      ctx.fillStyle = "#b5d0ff";
      ctx.fillRect(x + 6, y - 5, 1, 5);
      break;
    }
  }
}

/** The pose for what the character is doing (sprites.ts has the frames). */
/** After this long idle an agent dozes; after this long waiting for you it gets impatient. */
const DOZE_MS = 3 * 60_000;
const IMPATIENT_MS = 2 * 60_000;

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
    case "WAITING_INPUT": {
      // Waves now and then to be noticed; twice as fast once it has waited a while.
      const period = now - entity.activitySince > IMPATIENT_MS ? 300 : 600;
      return Math.floor(now / period + offset) % 4 === 3 ? (sit ? "sit" : "stand") : sit ? "raise" : "raiseStand";
    }
    case "ERROR":
      return sit ? "worried" : "worriedStand";
    case "IDLE":
      // A long idle puts the coffee down: dozing.
      if (now - entity.activitySince > DOZE_MS) return sit ? "sit" : "stand";
      return sit ? "coffee" : "coffeeStand";
    case "DONE":
      return "celebrate";
  }
}

function drawIcon(ctx: CanvasRenderingContext2D, name: string, x: number, y: number) {
  const sprite = spriteCanvas(scale2x(ICONS[name]), ICON_PALETTE, `icon2x:${name}`);
  if (sprite) ctx.drawImage(sprite, x, y, 7, 7);
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
  const dozing = activity === "IDLE" && now - entity.activitySince > DOZE_MS;
  const name = silent ? "hourglass" : dozing ? "zzz" : BUBBLE_ICON[activity];
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
  // Soft drop shadow, then a border whose color says how urgent it is.
  ctx.fillStyle = "rgba(0,0,0,0.3)";
  ctx.fillRect(bx, by + 11, 12, 1);
  ctx.fillRect(bx + 12, by + 1, 1, 10);
  ctx.fillStyle = silent
    ? "#b07a2a"
    : activity === "WAITING_PERMISSION" || activity === "ERROR"
      ? "#d43c3c"
      : activity === "WAITING_INPUT"
        ? "#d9792b"
        : "#2b2f3a";
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

type Drawable = { y: number; furniture?: Furniture; entity?: Entity; pet?: OfficePet };

/** The cat, in HD like the characters, mirrored when it walks left. */
function drawPet(ctx: CanvasRenderingContext2D, pet: OfficePet, now: number) {
  const frame = pet.frame(now);
  const rows = PET_FRAMES[frame];
  const sprite = spriteCanvas(scale2x(rows), PET_PALETTE, `pet:${frame}`, "#14161e", true);
  ctx.fillStyle = "rgba(0,0,0,0.28)";
  ctx.fillRect(Math.round(pet.x) - 4, Math.round(pet.y), 8, 1.5);
  if (!sprite) return;
  const w = sprite.width / ART;
  const h = sprite.height / ART;
  const x = Math.round((pet.x - w / 2) * ART) / ART;
  const y = Math.round((pet.y - h + 1) * ART) / ART;
  if (pet.facing < 0) {
    ctx.save();
    ctx.translate(x + w, y);
    ctx.scale(-1, 1);
    ctx.drawImage(sprite, 0, 0, w, h);
    ctx.restore();
  } else ctx.drawImage(sprite, x, y, w, h);
}

/** How far in the user can zoom, relative to the whole-office view. */
export const MAX_ZOOM = 4;

export class OfficeRenderer {
  private staticLayer: HTMLCanvasElement | null = null;
  private art: CanvasRenderingContext2D | null | undefined;
  private theme: OfficeTheme = "modern";
  private phase: DayPhase = "night";
  private palette: WorldTheme | null = null;
  private pet: OfficePet | null = null;
  private lastNow = 0;
  /** Outer glazing tiles, for the rain. */
  private windows: [number, number][] = [];
  private readonly looks = new Map<string, CharacterLook>();
  private readonly textWidths = new Map<string, number>();
  private readonly drawables: Drawable[] = [];
  scale = 2;
  offsetX = 0;
  offsetY = 0;
  /** User zoom on top of the scale that fits the whole office (1 = fit). */
  zoom = 1;
  /** Layout point the view is centered on while zoomed in (null = the office center). */
  private focus: { x: number; y: number } | null = null;
  private fitScale = 1;
  private viewW = 0;
  private viewH = 0;

  constructor(private readonly layout: OfficeLayout) {}

  /** The scale for a zoom: the fitted scale times the zoom, in whole or half steps. */
  private scaleFor(zoom: number) {
    return zoom === 1 ? this.fitScale : Math.max(this.fitScale, Math.round(this.fitScale * zoom * 2) / 2);
  }

  private fit(width: number, height: number) {
    this.viewW = width;
    this.viewH = height;
    const lw = this.layout.width * TILE;
    const lh = this.layout.height * TILE;
    const raw = Math.min(width / lw, height / lh);
    this.fitScale = raw < 1 ? raw : Math.max(1, Math.floor(raw * 2) / 2);
    this.scale = this.scaleFor(this.zoom);
    // Centered when it fits; otherwise around the focus, never past an edge.
    const place = (view: number, size: number, center: number) =>
      size <= view ? Math.floor((view - size) / 2) : Math.round(Math.min(0, Math.max(view - size, view / 2 - center * this.scale)));
    this.offsetX = place(width, lw * this.scale, this.focus?.x ?? lw / 2);
    this.offsetY = place(height, lh * this.scale, this.focus?.y ?? lh / 2);
  }

  /** Zooms by a factor keeping the layout point under (px, py) in place (device pixels). */
  zoomAt(factor: number, px: number, py: number) {
    const zoom = Math.min(MAX_ZOOM, Math.max(1, this.zoom * factor));
    const p = this.toLayout(px, py);
    const scale = this.scaleFor(zoom);
    this.zoom = zoom;
    this.focus = zoom === 1 ? null : { x: p.x + (this.viewW / 2 - px) / scale, y: p.y + (this.viewH / 2 - py) / scale };
    this.fit(this.viewW, this.viewH);
  }

  /** Moves the view by a drag of (dx, dy) device pixels. */
  panBy(dx: number, dy: number) {
    if (this.zoom === 1) return;
    const center = this.toLayout(this.viewW / 2, this.viewH / 2);
    this.focus = { x: center.x - dx / this.scale, y: center.y - dy / this.scale };
    this.fit(this.viewW, this.viewH);
  }

  /** Back to the whole office. */
  resetView() {
    this.zoom = 1;
    this.focus = null;
    this.fit(this.viewW, this.viewH);
  }

  /** Brings a layout point into view if it is outside (or too close to) the edges. */
  reveal(x: number, y: number) {
    const s = this.toScreen(x, y);
    const margin = 48 * Math.max(1, this.scale / 2);
    if (s.x < margin || s.y < margin || s.x > this.viewW - margin || s.y > this.viewH - margin) {
      this.focus = { x, y };
      this.fit(this.viewW, this.viewH);
    }
  }

  toLayout(px: number, py: number) {
    return { x: (px - this.offsetX) / this.scale, y: (py - this.offsetY) / this.scale };
  }

  /** Screen position (device pixels) of a layout point. */
  toScreen(x: number, y: number) {
    return { x: this.offsetX + x * this.scale, y: this.offsetY + y * this.scale };
  }

  private look(entity: Entity, agent: AgentState | undefined, providers: Record<string, ProviderInfo>, custom: Appearance = {}): CharacterLook {
    const accent = (agent && providers[agent.provider]?.descriptor.accentColor) || "#9aa3b5";
    const cacheKey = `${entity.key}|${accent}|${entity.isMain}|${custom.style}|${custom.hair}|${custom.skin}|${custom.shirt}|${custom.accessory}`;
    let look = this.looks.get(cacheKey);
    if (!look) {
      look = characterLook(entity.key, accent, !entity.isMain, custom);
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

  /** Rain running down the outer glazing, and neon signs that flicker now and then. */
  private drawWeather(ctx: CanvasRenderingContext2D, now: number) {
    ctx.fillStyle = "rgba(170,210,255,0.45)";
    for (const [tx, ty] of this.windows) {
      for (let i = 0; i < 3; i++) {
        const n = hashString(`rain:${tx}:${i}`);
        const fall = (now / (28 + (n % 14)) + (n % 97)) % 10;
        if (fall > 7.5) continue;
        ctx.fillRect(tx * TILE + (n % 31) / 2, ty * TILE + 4 + fall, 0.5, 1.5);
      }
    }
    for (const d of this.layout.decorations) {
      if (d.kind !== "neon" || hashString(`flicker:${Math.floor(now / 70)}`) % 29 !== 0) continue;
      ctx.fillStyle = "rgba(7,6,15,0.7)";
      ctx.fillRect(d.x * TILE, d.y * TILE + 4, TILE, 8);
    }
  }

  /** The offscreen canvas the world is drawn on at ART density (null without a DOM). */
  private artContext(): CanvasRenderingContext2D | null {
    if (this.art === undefined) {
      this.art = null;
      if (typeof document !== "undefined") {
        const canvas = document.createElement("canvas");
        canvas.width = this.layout.width * TILE * ART;
        canvas.height = this.layout.height * TILE * ART;
        this.art = canvas.getContext("2d");
      }
    }
    return this.art;
  }

  draw(screen: CanvasRenderingContext2D, width: number, height: number, input: RenderInput): Hitbox[] {
    const { scene, agents, providers, now } = input;
    this.fit(width, height);
    const theme = input.theme ?? "modern";
    const phase = dayPhase(new Date(now).getHours());
    if (theme !== this.theme || phase !== this.phase) {
      this.theme = theme;
      this.phase = phase;
      this.staticLayer = null;
      this.palette = null;
    }
    T = this.palette ?? (this.palette = atPhase(THEMES[theme], phase));
    if (!this.staticLayer) {
      this.staticLayer = drawStatic(this.layout, scene);
      const { grid } = scene;
      this.windows = windowTiles(this.layout, (tx, ty) =>
        tx < 0 || ty < 0 || tx >= grid.width || ty >= grid.height ? "void" : grid.cells[ty * grid.width + tx],
      );
    }
    const lw = this.layout.width * TILE;
    const lh = this.layout.height * TILE;

    screen.setTransform(1, 0, 0, 1, 0, 0);
    screen.fillStyle = T.backdrop;
    screen.fillRect(0, 0, width, height);
    screen.imageSmoothingEnabled = false;
    // The world is drawn at ART density on its own canvas and scaled as one
    // image, so half-pixel details stay crisp. Without a DOM (tests) it is
    // drawn straight onto the screen.
    const art = this.artContext();
    let ctx = art ?? screen;
    if (art) {
      art.imageSmoothingEnabled = false;
      art.setTransform(ART, 0, 0, ART, 0, 0);
    } else screen.setTransform(this.scale, 0, 0, this.scale, this.offsetX, this.offsetY);
    if (this.staticLayer) ctx.drawImage(this.staticLayer, 0, 0, lw, lh);
    if (T.weather) this.drawWeather(ctx, now);

    // Lit screens spill their color onto the desk and floor around them.
    ctx.globalCompositeOperation = "lighter";
    for (const f of this.layout.furniture) {
      if (!f.seatId) continue;
      const occupant = scene.occupant(f.seatId);
      const activity = occupant && !occupant.celebrating ? occupant.activity : null;
      const rgb = activity && GLOW[f.kind === "console" ? "RUNNING_COMMAND" : activity];
      const glow = rgb ? glowSprite(rgb) : null;
      if (!glow) continue;
      ctx.drawImage(glow, f.x * TILE + (f.kind === "testBench" ? 8 : 5) - 22, f.y * TILE - 2 - 14);
      if (f.kind !== "testBench") ctx.drawImage(glow, (f.x + f.w) * TILE - 5 - 22, f.y * TILE - 2 - 14);
    }
    ctx.globalCompositeOperation = "source-over";

    const entities = [...scene.entities.values()];
    const hitboxes: Hitbox[] = [];
    const focusKey = input.hoverKey ?? input.selectedKey;
    const focusAgent = focusKey ? agents[focusKey] : undefined;
    // The focused agent's team: its lead and every subagent of that lead.
    const familyRoot = focusAgent ? (focusAgent.parentKey ?? focusAgent.key) : null;
    const filter = input.filter;
    const shown = (e: Entity) => !filter || (filter.kind === "attention" ? ATTENTION.has(e.activity) : e.team === filter.team);

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

    // Selection / attention rings under characters; a foot shadow for anyone standing.
    for (const e of entities) {
      if ((!e.sitting || e.path.length) && e.fadeMs === 0) {
        ctx.fillStyle = "rgba(0,0,0,0.3)";
        ctx.fillRect(Math.round(e.x) - 4, Math.round(e.y), 9, 2);
        ctx.fillRect(Math.round(e.x) - 3, Math.round(e.y) + 2, 7, 1);
      }
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
    if (!this.pet) this.pet = new OfficePet(scene.grid, this.layout.entrance);
    this.pet.step(this.lastNow ? now - this.lastNow : 0, now, scene.entities.values());
    this.lastNow = now;
    drawables.push({ y: this.pet.y, pet: this.pet });
    drawables.sort((a, b) => a.y - b.y);
    for (const d of drawables) {
      if (d.pet) {
        drawPet(ctx, d.pet, now);
        continue;
      }
      if (d.furniture) {
        const f = d.furniture;
        const occupant = f.seatId ? scene.occupant(f.seatId) : undefined;
        const activity = occupant && !occupant.celebrating ? occupant.activity : null;
        drawFurniture(ctx, f, now, activity);
        continue;
      }
      const e = d.entity!;
      const agent = agents[e.key];
      const sprite = characterCanvas(this.look(e, agent, providers, input.appearance?.[e.team]), frameFor(e, now));
      const typing = !e.path.length && e.sitting && e.activity === "CODING";
      const jump = e.celebrating ? Math.round(Math.abs(Math.sin(now / 130)) * 4) : 0;
      const bob = typing ? Math.floor(now / 180) % 2 : 0;
      const fading = e.fadeMs > 0;
      if (fading) ctx.globalAlpha = Math.max(0, 1 - e.fadeMs / EXIT_FADE_MS);
      if (!shown(e)) ctx.globalAlpha *= 0.22;
      // HD frames are ART× the grid plus a 1 art-pixel outline; placed on the art-pixel grid.
      const px = Math.round((e.x - SPRITE_W / 2) * ART) / ART - 1 / ART;
      const py = Math.round((e.y - (SPRITE_H - 1) - bob - jump) * ART) / ART - 1 / ART;
      if (sprite) ctx.drawImage(sprite, px, py, sprite.width / ART, sprite.height / ART);
      ctx.globalAlpha = 1;
      if (fading) drawPoof(ctx, e.x, e.y, Math.min(1, e.fadeMs / EXIT_FADE_MS));
      if (e.celebrating && agent?.endedAt !== undefined) drawConfetti(ctx, e.x, e.y, now - agent.endedAt, hashString(e.key));
      // A subagent that finishes next to its lead gets a high five: a spark between them.
      const lead = e.celebrating && e.parentKey ? scene.entities.get(e.parentKey) : undefined;
      if (lead && Math.hypot(lead.x - e.x, lead.y - e.y) < 40 && Math.floor(now / 250) % 2) {
        const sx = Math.round((lead.x + e.x) / 2);
        const sy = Math.round(Math.min(lead.y, e.y) - 16);
        ctx.fillStyle = "#fff6c8";
        ctx.fillRect(sx - 0.5, sy - 3, 1, 6);
        ctx.fillRect(sx - 3, sy - 0.5, 6, 1);
        ctx.fillRect(sx - 2, sy - 2, 0.5, 0.5);
        ctx.fillRect(sx + 1.5, sy - 2, 0.5, 0.5);
        ctx.fillRect(sx - 2, sy + 1.5, 0.5, 0.5);
        ctx.fillRect(sx + 1.5, sy + 1.5, 0.5, 0.5);
      }
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
      if ((!important && !focused) || !shown(e)) continue;
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

    if (art) {
      screen.setTransform(1, 0, 0, 1, 0, 0);
      screen.drawImage(art.canvas, this.offsetX, this.offsetY, lw * this.scale, lh * this.scale);
    }
    if (T.vignette) {
      // Film-frame edges: darkest at the corners of the view, clear in the middle.
      screen.setTransform(1, 0, 0, 1, 0, 0);
      const r = Math.hypot(width, height) / 2;
      const g = screen.createRadialGradient(width / 2, height / 2, r * 0.45, width / 2, height / 2, r);
      g.addColorStop(0, "rgba(0,0,0,0)");
      g.addColorStop(1, "rgba(0,0,0,0.55)");
      screen.fillStyle = g;
      screen.fillRect(0, 0, width, height);
    }
    ctx = screen;

    // Screen-space text (crisp at any scale).
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.textBaseline = "middle";
    // Text grows with the zoom only up to a point, so labels stay labels.
    const ts = Math.min(this.scale, this.fitScale * 1.75);
    const roomFont = Math.max(9, Math.round(5 * ts));
    ctx.font = `600 ${roomFont}px "Segoe UI", system-ui, sans-serif`;
    for (const room of this.layout.rooms) {
      const x = this.offsetX + (room.rect.x + 1) * TILE * this.scale + 4;
      const y = this.offsetY + (room.rect.y + 1) * TILE * this.scale + roomFont / 2 + 3;
      const text = room.name.toUpperCase();
      ctx.fillStyle = "rgba(15,17,23,0.55)";
      ctx.fillRect(x - 3, y - roomFont / 2 - 2, this.measure(ctx, text) + 6, roomFont + 4);
      ctx.fillStyle = "#e7e9ef";
      ctx.fillText(text, x, y);
    }

    // Team name plates on the desk rows.
    const tagFont = Math.max(10, Math.round(5.5 * ts));
    const half = Math.round(tagFont / 2) + 3;
    ctx.font = `700 ${tagFont}px "Segoe UI", system-ui, sans-serif`;
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
      ctx.fillStyle = T.plate.shadow;
      ctx.fillRect(x - 1, y - half - 2, w + 2, half * 2 + 4);
      ctx.fillStyle = T.plate.body;
      ctx.fillRect(x, y - half - 1, w, half * 2 + 2);
      ctx.fillStyle = T.plate.bar;
      ctx.fillRect(x, y - half - 1, 3, half * 2 + 2);
      ctx.fillStyle = T.plate.text;
      ctx.fillText(text, x + 7, y + 0.5);
    }

    if (ceoDesk && input.pendingApprovals > 0) {
      const x = this.offsetX + (ceoDesk.x * TILE + 14) * this.scale;
      const y = this.offsetY + (ceoDesk.y * TILE - 8) * this.scale;
      ctx.fillStyle = "#ff4d4d";
      ctx.beginPath();
      ctx.arc(x, y, half, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = "#ffffff";
      ctx.textAlign = "center";
      ctx.fillText(String(input.pendingApprovals), x, y + 0.5);
      ctx.textAlign = "left";
    }

    // Name tags: leads always, subagents when few or focused.
    const labelAll = entities.length <= 16;
    ctx.font = `600 ${tagFont}px "Segoe UI", system-ui, sans-serif`;
    const placed: { l: number; t: number; r: number; b: number }[] = [];
    const labelled = entities
      .filter((e) => {
        const agent = agents[e.key];
        const focused = e.key === input.selectedKey || e.key === input.hoverKey;
        return agent && e.fadeMs === 0 && (agent.isMain || labelAll || focused) && (focused || shown(e));
      })
      .sort((a, b) => b.y - a.y);
    for (const e of labelled) {
      const agent = agents[e.key];
      const focused = e.key === input.selectedKey || e.key === input.hoverKey;
      const provider = providers[agent.provider]?.descriptor;
      const badge = provider?.badge ?? "?";
      // A project's main agents go by the nickname the user gave them.
      const full = (agent.isMain && input.appearance?.[e.team]?.nickname) || agent.name;
      const name = full.length > 18 ? `${full.slice(0, 17)}…` : full;
      const cx = this.offsetX + e.x * this.scale;
      // Above the bubble when there is one, else just above the head.
      let cy = this.offsetY + (e.y - (withBubble.has(e.key) ? 36 : 23)) * this.scale;
      const badgeW = this.measure(ctx, badge) + 8;
      const nameW = this.measure(ctx, name) + 8;
      const total = badgeW + nameW;
      // Keep the tag on screen.
      const left = Math.round(Math.min(Math.max(cx - total / 2, 2), width - total - 2));
      cy = Math.max(cy, half + 2);
      // Nudge labels upwards until they no longer overlap one already drawn.
      for (let tries = 0; tries < 4; tries++) {
        const hit = placed.some((p) => left < p.r && left + total > p.l && cy - half < p.b && cy + half > p.t);
        if (!hit) break;
        cy -= half * 2 + 1;
      }
      placed.push({ l: left, t: cy - half, r: left + total, b: cy + half });
      ctx.globalAlpha = agent.ended ? 0.6 : 1;
      ctx.fillStyle = provider?.accentColor ?? "#9aa3b5";
      ctx.fillRect(left, cy - half, badgeW, half * 2);
      ctx.fillStyle = "#101218";
      ctx.fillText(badge, left + 4, cy);
      ctx.fillStyle = focused ? "rgba(242,201,76,0.95)" : "rgba(16,18,24,0.82)";
      ctx.fillRect(left + badgeW, cy - half, nameW, half * 2);
      ctx.fillStyle = focused ? "#101218" : "#f3f4f7";
      ctx.fillText(name, left + badgeW + 4, cy);
      ctx.globalAlpha = 1;
    }

    // Agents that need you but are outside the zoomed view: a marker on the
    // edge pointing at them (click it to go there).
    ctx.font = `700 ${tagFont}px "Segoe UI", system-ui, sans-serif`;
    ctx.textAlign = "center";
    const r = half + 3;
    for (const e of entities) {
      if (e.fadeMs > 0 || !ATTENTION.has(e.activity) || !shown(e)) continue;
      const p = this.toScreen(e.x, e.y - 8);
      if (p.x >= 0 && p.y >= 0 && p.x <= width && p.y <= height) continue;
      const cx = Math.min(Math.max(p.x, r + 6), width - r - 6);
      const cy = Math.min(Math.max(p.y, r + 6), height - r - 6);
      const angle = Math.atan2(p.y - cy, p.x - cx);
      ctx.fillStyle = e.activity === "WAITING_INPUT" ? "#f2994a" : "#ff4d4d";
      ctx.beginPath();
      ctx.moveTo(cx + Math.cos(angle) * (r + 6), cy + Math.sin(angle) * (r + 6));
      ctx.lineTo(cx + Math.cos(angle + 0.6) * r, cy + Math.sin(angle + 0.6) * r);
      ctx.lineTo(cx + Math.cos(angle - 0.6) * r, cy + Math.sin(angle - 0.6) * r);
      ctx.fill();
      ctx.beginPath();
      ctx.arc(cx, cy, r, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = "#ffffff";
      ctx.fillText(e.activity === "WAITING_INPUT" ? "?" : "!", cx, cy + 0.5);
      hitboxes.push({ key: e.key, kind: "offscreen", x: cx - r, y: cy - r, w: r * 2, h: r * 2 });
    }
    ctx.textAlign = "left";
    return hitboxes;
  }
}
