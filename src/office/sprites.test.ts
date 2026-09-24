import { describe, expect, it } from "vitest";
import { CHARACTER_FRAMES, characterLook, characterRows, HAIR_STYLES, ICON_PALETTE, ICONS, shade, SPRITE_H, SPRITE_W } from "./sprites";

describe("character sprites", () => {
  it("every pose of every hair style is a full 12 × 16 grid of known colors", () => {
    const look = characterLook("agent", "#d97757");
    for (const style of HAIR_STYLES) {
      for (const frame of CHARACTER_FRAMES) {
        const rows = characterRows(style, frame);
        expect(rows.length, `${style}:${frame}`).toBe(SPRITE_H);
        for (const row of rows) {
          expect(row.length, `${style}:${frame}`).toBe(SPRITE_W);
          for (const cell of row) if (cell !== ".") expect(look.palette[cell], `${style}:${frame} "${cell}"`).toBeDefined();
        }
      }
    }
  });

  it("poses really differ from standing", () => {
    for (const frame of CHARACTER_FRAMES.filter((f) => f !== "stand")) {
      expect(characterRows("short", frame), frame).not.toEqual(characterRows("short", "stand"));
    }
  });

  it("looks are stable per agent and subagents wear a lighter shade of the lead's color", () => {
    const a = characterLook("claude:s1:main", "#d97757");
    expect(characterLook("claude:s1:main", "#d97757")).toEqual(a);
    expect(a.palette.c).toBe("#d97757");
    const sub = characterLook("claude:s1:sub", "#d97757", true);
    expect(sub.palette.c).not.toBe("#d97757");
    expect(shade("#000000", 1.5)).toBe("#808080");
    expect(shade("#ffffff", 0.5)).toBe("#808080");
  });

  it("icons use only palette colors", () => {
    for (const [name, rows] of Object.entries(ICONS)) {
      for (const row of rows) for (const cell of row) if (cell !== ".") expect(ICON_PALETTE[cell], name).toBeDefined();
    }
  });
});
