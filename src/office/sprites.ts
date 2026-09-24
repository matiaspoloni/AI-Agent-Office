// Original pixel-art sprites, defined as character grids and rendered once
// per palette into cached offscreen canvases. No third-party art or logos.
//
// Characters are 12 × 16 cells, feet on the last row. A frame is composed
// from a head (hair style), a torso/arms pose and legs, plus small patches
// for poses that move the arms (raised hand, celebrating, hands on head).
// Palette keys: h/H hair (and its shade), q cap, s skin, m mouth, e eyes,
// c/C shirt (provider accent) and its shade, p trousers, f shoes, w paper,
// k ink, u mug, d sweat drop.

export type Palette = Record<string, string>;

export const SPRITE_W = 12;
export const SPRITE_H = 16;

export type HairStyle = "short" | "long" | "bun" | "cap";
export const HAIR_STYLES: HairStyle[] = ["short", "long", "bun", "cap"];

const HEAD: Record<HairStyle, string[]> = {
  short: [
    "....hhhh....",
    "...hhhhhh...",
    "..hhhhhhhh..",
    "..hssssssh..",
    "..sesssses..",
    "..ssssssss..",
    "...ssmmss...",
    "....ssss....",
  ],
  long: [
    "....hhhh....",
    "...hhhhhh...",
    "..hhhhhhhh..",
    ".hhsssssshh.",
    ".hsessssesh.",
    ".hssssssssh.",
    ".hhssmmsshh.",
    "....ssss....",
  ],
  bun: [
    ".....HH.....",
    "...hhhhhh...",
    "..hhhhhhhh..",
    "..hssssssh..",
    "..sesssses..",
    "..ssssssss..",
    "...ssmmss...",
    "....ssss....",
  ],
  cap: [
    "...qqqqqq...",
    "..qqqqqqqq..",
    "..qqqqqqqqq.",
    "..hssssssh..",
    "..sesssses..",
    "..ssssssss..",
    "...ssmmss...",
    "....ssss....",
  ],
};

const TORSO: Record<"rest" | "typeA" | "typeB" | "read" | "coffee" | "walkA" | "walkB", string[]> = {
  rest: ["..cccccccc..", ".cccccccccc.", ".sccCCCCccs.", ".sccccccccs."],
  // Hands moving on the keyboard, one up, one down.
  typeA: ["..cccccccc..", ".cccccccccc.", ".cscCCCCccc.", ".cccccccscc."],
  typeB: ["..cccccccc..", ".cccccccccc.", ".cccCCCCcsc.", ".ccsccccccc."],
  read: ["..cccccccc..", ".ccwwwwwwcc.", ".cswkkkkwsc.", ".ccwwwwwwcc."],
  coffee: ["..cccccccc..", ".ccccccccuu.", ".sccCCCCcsu.", ".scccccccc.."],
  walkA: ["..cccccccc..", ".cccccccccc.", ".sccCCCCccc.", "..cccccccss."],
  walkB: ["..cccccccc..", ".cccccccccc.", ".cccCCCCccs.", ".sscccccccc."],
};

const LEGS: Record<"stand" | "walkA" | "walkB" | "sit", string[]> = {
  stand: ["..pppppppp..", "..ppp..ppp..", "..ppp..ppp..", "..fff..fff.."],
  walkA: ["..pppppppp..", "..ppp...pp..", ".ppp....ppp.", ".fff.....ff."],
  walkB: ["..pppppppp..", "..pp...ppp..", ".ppp....ppp.", ".ff.....fff."],
  sit: ["..pppppppp..", "..pppppppp..", "..ff....ff..", "............"],
};

type Patch = [row: number, col: number, cell: string];

const RIGHT_ARM_UP: Patch[] = [
  [2, 11, "s"],
  [3, 11, "s"],
  [4, 11, "s"],
  [5, 11, "s"],
  [6, 11, "c"],
  [7, 11, "c"],
  [8, 10, "c"],
  [10, 10, "."],
  [11, 10, "."],
];
const LEFT_ARM_UP: Patch[] = RIGHT_ARM_UP.map(([r, c, v]) => [r, SPRITE_W - 1 - c, v]);

const PATCHES = {
  none: [] as Patch[],
  // Hand on the chin.
  think: [
    [7, 8, "s"],
    [7, 9, "s"],
    [8, 9, "c"],
    [8, 10, "c"],
    [9, 10, "c"],
    [10, 10, "."],
    [11, 10, "."],
  ] as Patch[],
  raise: RIGHT_ARM_UP,
  celebrate: [...RIGHT_ARM_UP, ...LEFT_ARM_UP],
  // Hands on the head and a drop of sweat.
  worried: [
    [2, 1, "s"],
    [3, 1, "s"],
    [4, 1, "c"],
    [5, 1, "c"],
    [6, 1, "c"],
    [7, 1, "c"],
    [10, 1, "."],
    [11, 1, "."],
    [2, 10, "s"],
    [3, 10, "s"],
    [4, 10, "c"],
    [5, 10, "c"],
    [6, 10, "c"],
    [7, 10, "c"],
    [10, 10, "."],
    [11, 10, "."],
    [5, 9, "d"],
  ] as Patch[],
};

/** Every pose the renderer can ask for: torso, legs and arm patch. */
const POSES = {
  stand: ["rest", "stand", "none"],
  walkA: ["walkA", "walkA", "none"],
  walkB: ["walkB", "walkB", "none"],
  sit: ["rest", "sit", "none"],
  typeA: ["typeA", "sit", "none"],
  typeB: ["typeB", "sit", "none"],
  read: ["read", "sit", "none"],
  readStand: ["read", "stand", "none"],
  think: ["rest", "sit", "think"],
  thinkStand: ["rest", "stand", "think"],
  raise: ["rest", "sit", "raise"],
  raiseStand: ["rest", "stand", "raise"],
  celebrate: ["rest", "stand", "celebrate"],
  worried: ["rest", "sit", "worried"],
  worriedStand: ["rest", "stand", "worried"],
  coffee: ["coffee", "sit", "none"],
  coffeeStand: ["coffee", "stand", "none"],
} as const satisfies Record<string, [keyof typeof TORSO, keyof typeof LEGS, keyof typeof PATCHES]>;

export type CharacterFrame = keyof typeof POSES;
export const CHARACTER_FRAMES = Object.keys(POSES) as CharacterFrame[];

/** The grid of one character frame. */
export function characterRows(style: HairStyle, frame: CharacterFrame): string[] {
  const [torso, legs, patch] = POSES[frame];
  const rows = [...HEAD[style], ...TORSO[torso], ...LEGS[legs]].map((r) => r.split(""));
  for (const [r, c, v] of PATCHES[patch]) rows[r][c] = v;
  return rows.map((r) => r.join(""));
}

export const ICONS: Record<string, string[]> = {
  alert: ["...r...", "..rrr..", "..rrr..", "..rrr..", "...r...", ".......", "...r..."],
  question: [".ooo...", "o...o..", "....o..", "...o...", "..o....", ".......", "..o...."],
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
  o: "#f08a24",
  g: "#3fbf5f",
  k: "#2b2f3a",
  b: "#4c8df5",
};

const HAIR = ["#2b1d14", "#5a3825", "#8c5a2b", "#d9a55b", "#e6d3a3", "#b83c2e", "#3b3b44", "#7a7f8c"];
const SKIN = ["#f5d0b0", "#e9b88f", "#c98f65", "#9c6644", "#6e4630", "#f1c7a1"];
const CAPS = ["#3a4052", "#2f6f9f", "#8f3b4c", "#3f7d4f", "#6b5bd6"];

export function hashString(text: string): number {
  let h = 2166136261;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

export function shade(hex: string, amount: number): string {
  const n = parseInt(hex.slice(1), 16);
  const clamp = (v: number) => Math.max(0, Math.min(255, Math.round(v)));
  const mix = (v: number) => (amount <= 1 ? v * amount : v + (255 - v) * (amount - 1));
  const r = clamp(mix((n >> 16) & 255));
  const g = clamp(mix((n >> 8) & 255));
  const b = clamp(mix(n & 255));
  return `#${((r << 16) | (g << 8) | b).toString(16).padStart(6, "0")}`;
}

/** Looks derived from the agent key: the same agent always looks the same. */
export interface CharacterLook {
  style: HairStyle;
  palette: Palette;
  /** Cache key for rendered frames. */
  id: string;
}

/**
 * The shirt carries the provider's accent color; subagents wear a lighter
 * shade of their lead's color so a team reads as a team.
 */
export function characterLook(agentKey: string, accent: string, subagent = false): CharacterLook {
  const h = hashString(agentKey);
  const skin = SKIN[h % SKIN.length];
  const hair = HAIR[(h >>> 3) % HAIR.length];
  const shirt = subagent ? shade(accent, 1.35) : accent;
  const style = HAIR_STYLES[(h >>> 7) % HAIR_STYLES.length];
  return {
    style,
    id: `${agentKey}|${shirt}`,
    palette: {
      h: hair,
      H: shade(hair, 0.75),
      q: CAPS[(h >>> 11) % CAPS.length],
      s: skin,
      m: shade(skin, 0.78),
      e: "#1d1f27",
      c: shirt,
      C: shade(shirt, 0.75),
      p: "#343a4f",
      f: "#1f2230",
      w: "#f7f4ec",
      k: "#2b2f3a",
      u: "#f2efe8",
      d: "#6aa8ff",
    },
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

/** A character frame for one look (cached). */
export function characterCanvas(look: CharacterLook, frame: CharacterFrame): HTMLCanvasElement | null {
  const key = `${look.id}:${frame}`;
  const hit = cache.get(key);
  if (hit) return hit;
  return spriteCanvas(characterRows(look.style, frame), look.palette, key);
}

/** Drops cached frames (e.g. after a provider color change). */
export function clearSpriteCache() {
  cache.clear();
}
