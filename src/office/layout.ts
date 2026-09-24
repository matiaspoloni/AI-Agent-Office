// Data-driven office layout. Rooms, furniture, decorations and seats are
// plain data so that later phases can move furniture, unlock objects and
// apply upgrades without touching the renderer. No economy exists yet; the
// Perk/OfficeUpgrade types only reserve the shape.

export const TILE = 16;
export const GRID_W = 48;
export const GRID_H = 28;

export type RoomKind = "ceo" | "meeting" | "qa" | "desks" | "terminal" | "lounge";

export type Zone = "desk" | "meeting" | "qa" | "terminal" | "lounge" | "standing";

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Room {
  id: string;
  kind: RoomKind;
  name: string;
  rect: Rect; // outer rect, walls on the border
  doors: { x: number; y: number }[];
  floor: string; // floor style key
}

export type FurnitureKind =
  | "desk"
  | "ceoDesk"
  | "meetingTable"
  | "serverRack"
  | "console"
  | "testBench"
  | "sofa"
  | "coffee"
  | "plant"
  | "bookshelf"
  | "whiteboard"
  | "waterCooler";

export interface Furniture {
  id: string;
  kind: FurnitureKind;
  x: number;
  y: number;
  w: number;
  h: number;
  /** Furniture blocks walking unless false (e.g. wall-mounted whiteboard). */
  solid?: boolean;
  /** Seat that "uses" this furniture (desk ↔ chair) — lets the renderer light the monitor. */
  seatId?: string;
}

export interface Decoration {
  id: string;
  kind: "rug" | "poster" | "clock";
  x: number;
  y: number;
}

export interface Seat {
  id: string;
  zone: Zone;
  roomId: string;
  x: number; // tile
  y: number; // tile
  /** Pixel offset of the character's feet relative to the tile's bottom-center. */
  dx?: number;
  dy?: number;
  sitting: boolean;
}

/**
 * A group of desks that one project team shares (a row in the open space).
 * The scene assigns pods to teams at runtime; `label` is where the team's
 * name plate is drawn (tile coordinates).
 */
export interface DeskPod {
  id: string;
  roomId: string;
  seatIds: string[];
  label: { x: number; y: number };
}

// Reserved for later phases (office customization / progression).
export interface Perk {
  id: string;
  name: string;
  description: string;
}

export interface OfficeUpgrade {
  id: string;
  name: string;
  adds: Furniture[];
  requires?: string[];
}

export interface OfficeLayout {
  id: string;
  name: string;
  width: number;
  height: number;
  entrance: { x: number; y: number };
  corridors: Rect[];
  rooms: Room[];
  furniture: Furniture[];
  decorations: Decoration[];
  seats: Seat[];
  pods: DeskPod[];
}

function build(): OfficeLayout {
  const rooms: Room[] = [
    { id: "ceo", kind: "ceo", name: "CEO Office", rect: { x: 1, y: 1, w: 11, h: 8 }, doors: [{ x: 6, y: 8 }], floor: "wood" },
    { id: "meeting", kind: "meeting", name: "Meeting Room", rect: { x: 13, y: 1, w: 15, h: 8 }, doors: [{ x: 20, y: 8 }], floor: "carpetBlue" },
    { id: "qa", kind: "qa", name: "QA Lab", rect: { x: 29, y: 1, w: 18, h: 8 }, doors: [{ x: 37, y: 8 }], floor: "checker" },
    {
      id: "desks",
      kind: "desks",
      name: "Open Space",
      rect: { x: 1, y: 10, w: 29, h: 17 },
      doors: [{ x: 13, y: 10 }, { x: 29, y: 18 }],
      floor: "carpet",
    },
    { id: "terminal", kind: "terminal", name: "Terminal Room", rect: { x: 32, y: 10, w: 15, h: 8 }, doors: [{ x: 32, y: 13 }], floor: "metal" },
    { id: "lounge", kind: "lounge", name: "Lounge", rect: { x: 32, y: 19, w: 15, h: 8 }, doors: [{ x: 32, y: 22 }], floor: "warm" },
  ];

  const corridors: Rect[] = [
    { x: 0, y: 9, w: 47, h: 1 },
    { x: 30, y: 9, w: 2, h: 18 },
    { x: 32, y: 18, w: 15, h: 1 },
  ];

  const furniture: Furniture[] = [];
  const seats: Seat[] = [];
  const pods: DeskPod[] = [];

  // Open space: 4 rows x 6 desks, one row per project team. The chair is
  // above the desk; characters face the viewer.
  const deskColumns = [3, 7, 11, 15, 19, 23];
  const deskRows = [13, 17, 21, 25];
  deskRows.forEach((row, r) => {
    const seatIds: string[] = [];
    deskColumns.forEach((col, c) => {
      const seatId = `desk-${r}-${c}`;
      seatIds.push(seatId);
      furniture.push({ id: `desk-f-${r}-${c}`, kind: "desk", x: col, y: row, w: 2, h: 1, seatId });
      seats.push({ id: seatId, zone: "desk", roomId: "desks", x: col, y: row - 1, dx: 8, dy: 5, sitting: true });
    });
    // Name plate at the end of the row, beside the last chair.
    pods.push({ id: `pod-${r}`, roomId: "desks", seatIds, label: { x: deskColumns[deskColumns.length - 1] + 2, y: row - 1 } });
  });
  furniture.push({ id: "plant-os-1", kind: "plant", x: 27, y: 11, w: 1, h: 1 });
  furniture.push({ id: "plant-os-2", kind: "plant", x: 2, y: 11, w: 1, h: 1 });
  furniture.push({ id: "cooler", kind: "waterCooler", x: 27, y: 23, w: 1, h: 1 });

  // CEO office: desk with the approvals inbox, bookshelf, plant.
  furniture.push({ id: "ceo-desk", kind: "ceoDesk", x: 5, y: 4, w: 3, h: 1 });
  furniture.push({ id: "ceo-shelf", kind: "bookshelf", x: 2, y: 2, w: 2, h: 1 });
  furniture.push({ id: "ceo-plant", kind: "plant", x: 10, y: 2, w: 1, h: 1 });
  furniture.push({ id: "ceo-plant-2", kind: "plant", x: 10, y: 7, w: 1, h: 1 });

  // Meeting room: table with chairs on both long sides.
  furniture.push({ id: "meeting-table", kind: "meetingTable", x: 16, y: 4, w: 9, h: 2 });
  furniture.push({ id: "meeting-board", kind: "whiteboard", x: 18, y: 2, w: 5, h: 1, solid: false });
  [16, 18, 20, 22, 24].forEach((x, i) => {
    seats.push({ id: `meet-top-${i}`, zone: "meeting", roomId: "meeting", x, y: 3, dy: 5, sitting: true });
    seats.push({ id: `meet-bottom-${i}`, zone: "meeting", roomId: "meeting", x, y: 6, dy: 2, sitting: true });
  });

  // QA lab: test benches with chairs above them.
  [31, 35, 39, 43].forEach((x, i) => {
    const seatId = `qa-${i}`;
    furniture.push({ id: `qa-bench-${i}`, kind: "testBench", x, y: 5, w: 2, h: 1, seatId });
    seats.push({ id: seatId, zone: "qa", roomId: "qa", x, y: 4, dx: 8, dy: 5, sitting: true });
  });
  furniture.push({ id: "qa-board", kind: "whiteboard", x: 39, y: 2, w: 4, h: 1, solid: false });
  furniture.push({ id: "qa-plant", kind: "plant", x: 45, y: 7, w: 1, h: 1 });
  [32, 36, 40, 44].forEach((x, i) =>
    seats.push({ id: `qa-stand-${i}`, zone: "qa", roomId: "qa", x, y: 7, sitting: false }),
  );

  // Terminal room: racks along the wall, consoles with chairs.
  [34, 36, 38, 40, 42, 44].forEach((x, i) =>
    furniture.push({ id: `rack-${i}`, kind: "serverRack", x, y: 11, w: 1, h: 1 }),
  );
  [34, 38, 42].forEach((x, i) => {
    const seatId = `term-${i}`;
    furniture.push({ id: `console-${i}`, kind: "console", x, y: 15, w: 2, h: 1, seatId });
    seats.push({ id: seatId, zone: "terminal", roomId: "terminal", x, y: 14, dx: 8, dy: 5, sitting: true });
  });
  [35, 37, 39, 41, 43, 45].forEach((x, i) =>
    seats.push({ id: `term-stand-${i}`, zone: "terminal", roomId: "terminal", x, y: 12, sitting: false }),
  );

  // Lounge: sofas, coffee machine, standing spots.
  furniture.push({ id: "sofa-1", kind: "sofa", x: 34, y: 20, w: 3, h: 1 });
  furniture.push({ id: "sofa-2", kind: "sofa", x: 40, y: 20, w: 3, h: 1 });
  furniture.push({ id: "coffee", kind: "coffee", x: 45, y: 20, w: 1, h: 1 });
  furniture.push({ id: "lounge-plant", kind: "plant", x: 33, y: 25, w: 1, h: 1 });
  [34, 35, 36, 40, 41, 42].forEach((x, i) =>
    seats.push({ id: `sofa-${i}`, zone: "lounge", roomId: "lounge", x, y: 21, dy: -1, sitting: true }),
  );
  [
    [44, 22],
    [45, 23],
    [38, 23],
    [39, 24],
    [35, 24],
    [42, 24],
    [37, 25],
    [44, 25],
  ].forEach(([x, y], i) => seats.push({ id: `lounge-stand-${i}`, zone: "lounge", roomId: "lounge", x, y, sitting: false }));

  // Overflow standing spots along the corridors (used only when everything else is taken).
  for (let i = 0; i < 26; i++) {
    seats.push({ id: `corridor-${i}`, zone: "standing", roomId: "corridor", x: 2 + i, y: 9, sitting: false });
  }
  for (let i = 0; i < 8; i++) {
    seats.push({ id: `corridor-v-${i}`, zone: "standing", roomId: "corridor", x: 30, y: 11 + i * 2, sitting: false });
  }

  return {
    id: "default",
    name: "Default office",
    width: GRID_W,
    height: GRID_H,
    entrance: { x: 0, y: 9 },
    corridors,
    rooms,
    furniture,
    decorations: [
      { id: "clock", kind: "clock", x: 20, y: 1 },
      { id: "poster", kind: "poster", x: 8, y: 10 },
    ],
    seats,
    pods,
  };
}

export const DEFAULT_LAYOUT: OfficeLayout = build();

export type Cell = "void" | "wall" | "floor" | "door" | "corridor";

export interface Grid {
  width: number;
  height: number;
  cells: Cell[];
  roomAt: (string | null)[];
  walkable: boolean[];
}

/** Rasterizes the layout into cells + a walkability mask for path finding. */
export function buildGrid(layout: OfficeLayout): Grid {
  const { width, height } = layout;
  const cells: Cell[] = new Array(width * height).fill("void");
  const roomAt: (string | null)[] = new Array(width * height).fill(null);
  const idx = (x: number, y: number) => y * width + x;

  for (const c of layout.corridors) {
    for (let y = c.y; y < c.y + c.h; y++) for (let x = c.x; x < c.x + c.w; x++) cells[idx(x, y)] = "corridor";
  }
  for (const room of layout.rooms) {
    const { x, y, w, h } = room.rect;
    for (let ty = y; ty < y + h; ty++) {
      for (let tx = x; tx < x + w; tx++) {
        const border = tx === x || ty === y || tx === x + w - 1 || ty === y + h - 1;
        cells[idx(tx, ty)] = border ? "wall" : "floor";
        roomAt[idx(tx, ty)] = room.id;
      }
    }
    for (const d of room.doors) cells[idx(d.x, d.y)] = "door";
  }

  const walkable = cells.map((c) => c === "floor" || c === "door" || c === "corridor");
  for (const f of layout.furniture) {
    if (f.solid === false) continue;
    for (let y = f.y; y < f.y + f.h; y++) for (let x = f.x; x < f.x + f.w; x++) walkable[idx(x, y)] = false;
  }
  for (const s of layout.seats) walkable[idx(s.x, s.y)] = true;
  walkable[idx(layout.entrance.x, layout.entrance.y)] = true;
  return { width, height, cells, roomAt, walkable };
}

/** Breadth-first path on the tile grid (4-neighbour). Returns tiles from start (exclusive) to goal. */
export function findPath(
  grid: Grid,
  from: { x: number; y: number },
  to: { x: number; y: number },
): { x: number; y: number }[] {
  const { width, height, walkable } = grid;
  const start = from.y * width + from.x;
  const goal = to.y * width + to.x;
  if (start === goal) return [];
  const prev = new Int32Array(width * height).fill(-1);
  const queue: number[] = [start];
  prev[start] = start;
  for (let head = 0; head < queue.length; head++) {
    const cur = queue[head];
    if (cur === goal) break;
    const cx = cur % width;
    const cy = (cur - cx) / width;
    const neighbours = [
      [cx + 1, cy],
      [cx - 1, cy],
      [cx, cy + 1],
      [cx, cy - 1],
    ];
    for (const [nx, ny] of neighbours) {
      if (nx < 0 || ny < 0 || nx >= width || ny >= height) continue;
      const n = ny * width + nx;
      if (prev[n] !== -1 || (!walkable[n] && n !== goal)) continue;
      prev[n] = cur;
      queue.push(n);
    }
  }
  if (prev[goal] === -1) return [];
  const path: { x: number; y: number }[] = [];
  for (let cur = goal; cur !== start; cur = prev[cur]) path.push({ x: cur % width, y: Math.floor(cur / width) });
  return path.reverse();
}

/**
 * Checks a layout before it is used (the default one in tests; saved or
 * edited layouts later). Returns human-readable problems; empty = valid.
 */
export function validateLayout(layout: OfficeLayout): string[] {
  const problems: string[] = [];
  const { width, height } = layout;
  const inside = (x: number, y: number) => x >= 0 && y >= 0 && x < width && y < height;
  const unique = (kind: string, ids: string[]) => {
    const seen = new Set<string>();
    for (const id of ids) {
      if (seen.has(id)) problems.push(`duplicate ${kind} id ${id}`);
      seen.add(id);
    }
  };
  unique("room", layout.rooms.map((r) => r.id));
  unique("furniture", layout.furniture.map((f) => f.id));
  unique("seat", layout.seats.map((s) => s.id));
  unique("pod", layout.pods.map((p) => p.id));

  for (const room of layout.rooms) {
    const { x, y, w, h } = room.rect;
    if (!inside(x, y) || !inside(x + w - 1, y + h - 1)) problems.push(`room ${room.id} is outside the office`);
    for (const d of room.doors) {
      const onBorder = (d.x === x || d.x === x + w - 1 || d.y === y || d.y === y + h - 1) && d.x >= x && d.x < x + w && d.y >= y && d.y < y + h;
      if (!onBorder) problems.push(`door ${d.x},${d.y} of ${room.id} is not on its wall`);
    }
  }

  const grid = buildGrid(layout);
  const occupied = new Map<number, string>();
  for (const f of layout.furniture) {
    for (let y = f.y; y < f.y + f.h; y++)
      for (let x = f.x; x < f.x + f.w; x++) {
        if (!inside(x, y)) {
          problems.push(`furniture ${f.id} is outside the office`);
          continue;
        }
        const cell = grid.cells[y * width + x];
        if (cell !== "floor") problems.push(`furniture ${f.id} stands on ${cell} at ${x},${y}`);
        if (f.solid === false) continue;
        const other = occupied.get(y * width + x);
        if (other) problems.push(`furniture ${f.id} overlaps ${other}`);
        occupied.set(y * width + x, f.id);
      }
  }

  const seatIds = new Set(layout.seats.map((s) => s.id));
  for (const seat of layout.seats) {
    if (!inside(seat.x, seat.y)) {
      problems.push(`seat ${seat.id} is outside the office`);
      continue;
    }
    if (occupied.has(seat.y * width + seat.x)) problems.push(`seat ${seat.id} is inside furniture ${occupied.get(seat.y * width + seat.x)}`);
    if (findPath(grid, layout.entrance, seat).length === 0) problems.push(`seat ${seat.id} cannot be reached from the entrance`);
  }
  for (const pod of layout.pods) {
    for (const id of pod.seatIds) {
      if (!seatIds.has(id)) problems.push(`pod ${pod.id} lists unknown seat ${id}`);
      else if (layout.seats.find((s) => s.id === id)?.zone !== "desk") problems.push(`pod ${pod.id} seat ${id} is not a desk`);
    }
  }
  return problems;
}
