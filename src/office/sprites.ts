// Original pixel-art sprites, defined as character grids and rendered once
// per palette into cached offscreen canvases. No third-party art or logos.
//
// Characters are 12 × 16 cells, feet on the last row. A frame is composed
// from a head (hair style), a torso/arms pose and legs, plus small patches
// for poses that move the arms (raised hand, celebrating, hands on head).
// Palette keys: h/H/i hair (its shade and shine), q cap, s skin, m mouth, e eyes,
// c/C shirt (provider accent) and its shade, p trousers, f shoes, w paper,
// k ink, u mug, d sweat drop; accessories: g glasses, z/Z headphones, y/Y bow.

export type Palette = Record<string, string>;

export const SPRITE_W = 12;
export const SPRITE_H = 16;

export type HairStyle = "short" | "long" | "bun" | "cap";
export const HAIR_STYLES: HairStyle[] = ["short", "long", "bun", "cap"];

const HEAD: Record<HairStyle, string[]> = {
  short: [
    "....hhhh....",
    "...hiihhh...",
    "..hhhhhhhh..",
    "..hssssssh..",
    "..sesssses..",
    "..ssssssss..",
    "...ssmmss...",
    "....ssss....",
  ],
  long: [
    "....hhhh....",
    "...hiihhh...",
    "..hhhhhhhh..",
    ".hhsssssshh.",
    ".hsessssesh.",
    ".hssssssssh.",
    ".hhssmmsshh.",
    "....ssss....",
  ],
  bun: [
    ".....HH.....",
    "...hiihhh...",
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

export type Accessory = "none" | "glasses" | "headphones" | "beard" | "bow";
export const ACCESSORIES: Accessory[] = ["none", "glasses", "headphones", "beard", "bow"];

/** Accessories are patches on the head, applied before the pose so hands go over them. */
const ACCESSORY_PATCHES: Record<Accessory, Patch[]> = {
  none: [],
  // Frames around both eyes with a bridge.
  glasses: [
    [4, 2, "g"],
    [4, 4, "g"],
    [4, 5, "g"],
    [4, 6, "g"],
    [4, 7, "g"],
    [4, 9, "g"],
    [3, 3, "g"],
    [3, 8, "g"],
  ],
  // Band over the hair, cups over the ears.
  headphones: [
    [0, 3, "z"],
    [0, 4, "z"],
    [0, 5, "z"],
    [0, 6, "z"],
    [0, 7, "z"],
    [0, 8, "z"],
    [1, 2, "z"],
    [1, 9, "z"],
    [2, 1, "z"],
    [2, 10, "z"],
    [3, 1, "Z"],
    [4, 1, "Z"],
    [5, 1, "Z"],
    [3, 10, "Z"],
    [4, 10, "Z"],
    [5, 10, "Z"],
  ],
  // Beard in the hair's shade, leaving the mouth.
  beard: [
    [5, 2, "H"],
    [5, 9, "H"],
    [6, 3, "H"],
    [6, 4, "H"],
    [6, 7, "H"],
    [6, 8, "H"],
    [7, 4, "H"],
    [7, 5, "H"],
    [7, 6, "H"],
    [7, 7, "H"],
  ],
  // A bow on the side of the head.
  bow: [
    [0, 8, "y"],
    [0, 9, "Y"],
    [0, 10, "y"],
    [1, 8, "y"],
    [1, 10, "y"],
  ],
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
export function characterRows(style: HairStyle, frame: CharacterFrame, accessory: Accessory = "none"): string[] {
  const [torso, legs, patch] = POSES[frame];
  const rows = [...HEAD[style], ...TORSO[torso], ...LEGS[legs]].map((r) => r.split(""));
  for (const [r, c, v] of ACCESSORY_PATCHES[accessory]) rows[r][c] = v;
  for (const [r, c, v] of PATCHES[patch]) rows[r][c] = v;
  return rows.map((r) => r.join(""));
}

export const ICONS: Record<string, string[]> = {
  alert: ["..rrr..", "..rrr..", "..rrr..", "...r...", "...r...", ".......", "..rrr.."],
  question: [".oooo..", "oo..oo.", "....oo.", "...oo..", "..oo...", ".......", "..oo..."],
  error: ["rr...rr", ".rr.rr.", "..rrr..", "...r...", "..rrr..", ".rr.rr.", "rr...rr"],
  check: [".......", "......g", ".....gg", "g...gg.", "gg.gg..", ".ggg...", "..g...."],
  terminal: ["kkkkkkk", "k.....k", "kg....k", "k.g...k", "kg.gg.k", "k.....k", "kkkkkkk"],
  flask: ["..kkk..", "...k...", "...k...", "..kgk..", ".kgggk.", "kgggggk", "kkkkkkk"],
  doc: [".kkkk..", ".k..kk.", ".kbbbk.", ".k...k.", ".kbbbk.", ".k...k.", ".kkkkk."],
  code: ["..b.b..", ".b...b.", "b..k..b", "b..k..b", "b..k..b", ".b...b.", "..b.b.."],
  coffee: [".......", "..k.k..", ".......", "kkkkkk.", "kbbbbkk", "kbbbbk.", ".kkkk.."],
  zzz: ["kkkk...", "..k....", ".k.....", "kkkk...", "....kkk", ".....k.", "....kkk"],
  hourglass: ["kkkkkkk", ".koook.", "..kok..", "...k...", "..k.k..", ".k.o.k.", "kkkkkkk"],
};

export const ICON_PALETTE: Palette = {
  r: "#ff4d4d",
  o: "#f08a24",
  g: "#3fbf5f",
  k: "#2b2f3a",
  b: "#4c8df5",
};

export const HAIR = ["#2b1d14", "#5a3825", "#8c5a2b", "#d9a55b", "#e6d3a3", "#b83c2e", "#3b3b44", "#7a7f8c"];
export const SKIN = ["#f5d0b0", "#e9b88f", "#c98f65", "#9c6644", "#6e4630", "#f1c7a1"];
/** Shirt colors the user can pick instead of the provider's. */
export const SHIRTS = ["#d97757", "#4c8df5", "#3fbf5f", "#f2c94c", "#b58cff", "#3bd1c6", "#ff6b9a", "#e7e9ef"];
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
  accessory: Accessory;
  palette: Palette;
  /** Cache key for rendered frames. */
  id: string;
}

/** What the user chose for a project's agents; anything unset is derived from the agent key. */
export interface CustomLook {
  style?: HairStyle;
  hair?: string;
  skin?: string;
  shirt?: string;
  accessory?: Accessory;
}

/**
 * The shirt carries the provider's accent color (or the user's choice);
 * subagents wear a lighter shade of their lead's color so a team reads as a team.
 */
export function characterLook(agentKey: string, accent: string, subagent = false, custom: CustomLook = {}): CharacterLook {
  const h = hashString(agentKey);
  const skin = custom.skin ?? SKIN[h % SKIN.length];
  const hair = custom.hair ?? HAIR[(h >>> 3) % HAIR.length];
  const base = custom.shirt ?? accent;
  const shirt = subagent ? shade(base, 1.35) : base;
  const style = custom.style ?? HAIR_STYLES[(h >>> 7) % HAIR_STYLES.length];
  const accessory = custom.accessory ?? "none";
  return {
    style,
    accessory,
    id: `${agentKey}|${shirt}|${style}|${hair}|${skin}|${accessory}`,
    palette: {
      h: hair,
      H: shade(hair, 0.75),
      i: shade(hair, 1.3),
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
      g: "#1d1f27",
      z: "#2b2f3a",
      Z: "#6b7590",
      y: "#ff6b9a",
      Y: "#c94a73",
    },
  };
}

const cache = new Map<string, HTMLCanvasElement>();

/** Dark rim drawn around characters so they read on any floor. */
const OUTLINE = "#14161e";

/**
 * Scale2x (EPX): doubles a grid, rounding diagonals without inventing colors.
 * Cells outside the grid count as empty.
 */
export function scale2x(rows: readonly string[]): string[] {
  const at = (x: number, y: number) => (y >= 0 && y < rows.length && x >= 0 && x < rows[y].length ? rows[y][x] : ".");
  const out: string[] = [];
  for (let y = 0; y < rows.length; y++) {
    let top = "";
    let bottom = "";
    for (let x = 0; x < rows[y].length; x++) {
      const p = at(x, y);
      const a = at(x, y - 1);
      const b = at(x + 1, y);
      const c = at(x - 1, y);
      const d = at(x, y + 1);
      top += (c === a && c !== d && a !== b ? a : p) + (a === b && a !== c && b !== d ? b : p);
      bottom += (d === c && d !== b && c !== a ? c : p) + (b === d && b !== a && d !== c ? d : p);
    }
    out.push(top, bottom);
  }
  return out;
}

/** Materials that take light: everything but eyes, mouth, ink, glasses and the sweat drop. */
const SHADED = new Set(["h", "H", "i", "q", "s", "c", "C", "p", "f", "u", "w", "z", "Z", "y", "Y"]);
const EYE_SHINE = "#f5f7fa";

/**
 * Renders a grid sprite at 1 px per cell into a cached canvas. With an
 * outline color the canvas is 2 px larger and every empty cell next to a
 * filled one (4-neighbour) gets that color. `lit` adds light from the top
 * left: a highlight on each material's top edge, shade on its bottom and
 * right edges, and a shine in the eyes.
 */
export function spriteCanvas(
  rows: readonly string[],
  palette: Palette,
  cacheKey: string,
  outline?: string,
  lit = false,
): HTMLCanvasElement | null {
  const hit = cache.get(cacheKey);
  if (hit) return hit;
  if (typeof document === "undefined") return null;
  const pad = outline ? 1 : 0;
  const canvas = document.createElement("canvas");
  canvas.width = rows[0].length + pad * 2;
  canvas.height = rows.length + pad * 2;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  const cell = (x: number, y: number) => (y >= 0 && y < rows.length && x >= 0 && x < rows[y].length ? rows[y][x] : ".");
  const filled = (x: number, y: number) => !!palette[cell(x, y)];
  if (outline) {
    ctx.fillStyle = outline;
    for (let y = -1; y <= rows.length; y++)
      for (let x = -1; x <= rows[0].length; x++)
        if (!filled(x, y) && (filled(x - 1, y) || filled(x + 1, y) || filled(x, y - 1) || filled(x, y + 1))) ctx.fillRect(x + pad, y + pad, 1, 1);
  }
  rows.forEach((row, y) => {
    for (let x = 0; x < row.length; x++) {
      const k = row[x];
      let color = palette[k];
      if (!color) continue;
      if (lit && SHADED.has(k)) {
        if (cell(x, y - 1) !== k) color = shade(color, 1.16);
        else if (cell(x, y + 1) !== k || cell(x + 1, y) !== k) color = shade(color, 0.8);
      } else if (lit && k === "e" && cell(x, y - 1) !== "e" && cell(x - 1, y) !== "e") color = EYE_SHINE;
      ctx.fillStyle = color;
      ctx.fillRect(x + pad, y + pad, 1, 1);
    }
  });
  cache.set(cacheKey, canvas);
  return canvas;
}

/** A character frame for one look (cached), in HD: Scale2x, lit, with a fine outline. */
export function characterCanvas(look: CharacterLook, frame: CharacterFrame): HTMLCanvasElement | null {
  const key = `${look.id}:${frame}`;
  const hit = cache.get(key);
  if (hit) return hit;
  return spriteCanvas(scale2x(characterRows(look.style, frame, look.accessory)), look.palette, key, OUTLINE, true);
}

/** Drops cached frames (e.g. after a provider color change). */
export function clearSpriteCache() {
  cache.clear();
}
