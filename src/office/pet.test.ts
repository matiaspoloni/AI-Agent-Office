// @vitest-environment node
import { describe, expect, it } from "vitest";
import { buildGrid, DEFAULT_LAYOUT, TILE } from "./layout";
import { OfficePet } from "./pet";
import type { Entity } from "./scene";

describe("office cat", () => {
  it("curls up next to whoever has been stuck the longest", () => {
    const grid = buildGrid(DEFAULT_LAYOUT);
    const floor = (n: number) => {
      const idx = grid.cells.findIndex((c, i) => c === "floor" && grid.walkable[i] && i % grid.width > n);
      return { x: (idx % grid.width) * TILE + TILE / 2, y: Math.floor(idx / grid.width) * TILE + TILE - 3 };
    };
    const stuck = (key: string, since: number, at: { x: number; y: number }) =>
      ({ key, ...at, path: [], fadeMs: 0, activity: "WAITING_PERMISSION", activitySince: since }) as unknown as Entity;
    const older = stuck("older", 0, floor(4));
    const pet = new OfficePet(grid, DEFAULT_LAYOUT.entrance);
    for (let t = 0; t < 120_000; t += 50) pet.step(50, t, [stuck("newer", 5_000, floor(10)), older]);
    expect(pet.companion).toBe("older");
    expect(pet.resting).toBe(true);
    expect(Math.hypot(pet.x - older.x, pet.y - older.y)).toBeLessThan(TILE * 1.5);
    expect(pet.frame(0)).toBe("sleep");
  });
});
