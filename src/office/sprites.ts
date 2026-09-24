// Original pixel-art sprites, defined as character grids and rendered once
// per palette into cached offscreen canvases. No third-party art or logos.

export type Palette = Record<string, string>;

const CHARACTER_STAND = [
  "....hhhh....",
  "...hhhhhh...",
  "..hhhhhhhh..",
  "..hssssssh..",
  "..sesssses..",
  "..ssssssss..",
  "...ssmmss...",
  "....ssss....",
  "..cccccccc..",
  ".cccccccccc.",
  ".sccCCCCccs.",
  ".sccccccccs.",
  "..pppppppp..",
  "..ppp..ppp..",
  "..ppp..ppp..",
  "..fff..fff..",
];

const LEGS_WALK_A = ["..pppppppp..", "..ppp...pp..", ".ppp....ppp.", ".fff.....ff."];
const LEGS_WALK_B = ["..pppppppp..", "..pp...ppp..", ".ppp....ppp.", ".ff.....fff."];
const LEGS_SIT = ["..pppppppp..", "..pppppppp..", "..ff....ff..", "............"];

function withLegs(legs: string[]): string[] {
  return [...CHARACTER_STAND.slice(0, 12), ...legs];
}

export const CHARACTER_FRAMES = {
  stand: CHARACTER_STAND,
  walkA: withLegs(LEGS_WALK_A),
  walkB: withLegs(LEGS_WALK_B),
  sit: withLegs(LEGS_SIT),
} as const;

export type CharacterFrame = keyof typeof CHARACTER_FRAMES;

export const ICONS: Record<string, string[]> = {
  alert: ["...r...", "..rrr..", "..rrr..", "..rrr..", "...r...", ".......", "...r..."],
  error: ["r.....r", ".r...r.", "..r.r..", "...r...", "..r.r..", ".r...r.", "r.....r"],
  check: [".......", "......g", ".....g.", "g...g..", ".g.g...", "..g....", "......."],
  terminal: ["kkkkkkk", "k.....k", "kg....k", "k.g...k", "kg.gg.k", "k.....k", "kkkkkkk"],
  flask: ["..kkk..", "...k...", "...k...", "..kgk..", ".kgggk.", "kgggggk", "kkkkkkk"],
  doc: [".kkkk..", ".k..kk.", ".kbbbk.", ".k...k.", ".kbbbk.", ".k...k.", ".kkkkk."],
  code: ["..b.b..", ".b...b.", "b..k..b", "b..k..b", "b..k..b", ".b...b.", "..b.b.."],
  coffee: [".......", "..k.k..", ".......", "kkkkkk.", "kbbbbkk", "kbbbbk.", ".kkkk.."],
  zzz: ["kkkk...", "..k....", ".k.....", "kkkk...", "....kkk", ".....k.", "....kkk"],
};

export const ICON_PALETTE: Palette = {
  r: "#ff4d4d",
  g: "#3fbf5f",
  k: "#2b2f3a",
  b: "#4c8df5",
};

const HAIR = ["#2b1d14", "#5a3825", "#8c5a2b", "#d9a55b", "#e6d3a3", "#b83c2e", "#3b3b44", "#7a7f8c"];
const SKIN = ["#f5d0b0", "#e9b88f", "#c98f65", "#9c6644", "#6e4630", "#f1c7a1"];

export function hashString(text: string): number {
  let h = 2166136261;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

function shade(hex: string, amount: number): string {
  const n = parseInt(hex.slice(1), 16);
  const clamp = (v: number) => Math.max(0, Math.min(255, Math.round(v)));
  const r = clamp(((n >> 16) & 255) * amount);
  const g = clamp(((n >> 8) & 255) * amount);
  const b = clamp((n & 255) * amount);
  return `#${((r << 16) | (g << 8) | b).toString(16).padStart(6, "0")}`;
}

export function characterPalette(agentKey: string, accent: string): Palette {
  const h = hashString(agentKey);
  const skin = SKIN[h % SKIN.length];
  return {
    h: HAIR[(h >>> 3) % HAIR.length],
    s: skin,
    m: shade(skin, 0.78),
    e: "#1d1f27",
    c: accent,
    C: shade(accent, 0.75),
    p: "#343a4f",
    f: "#1f2230",
  };
}

const cache = new Map<string, HTMLCanvasElement>();

/** Renders a grid sprite at 1 px per cell into a cached canvas. */
export function spriteCanvas(rows: readonly string[], palette: Palette, cacheKey: string): HTMLCanvasElement | null {
  const hit = cache.get(cacheKey);
  if (hit) return hit;
  if (typeof document === "undefined") return null;
  const canvas = document.createElement("canvas");
  canvas.width = rows[0].length;
  canvas.height = rows.length;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  rows.forEach((row, y) => {
    for (let x = 0; x < row.length; x++) {
      const color = palette[row[x]];
      if (!color) continue;
      ctx.fillStyle = color;
      ctx.fillRect(x, y, 1, 1);
    }
  });
  cache.set(cacheKey, canvas);
  return canvas;
}
