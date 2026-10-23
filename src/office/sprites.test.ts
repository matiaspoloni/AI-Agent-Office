import { describe, expect, it } from "vitest";
import { ACCESSORIES, CHARACTER_FRAMES, characterLook, scale2x, characterRows, HAIR_STYLES, ICON_PALETTE, ICONS, shade, SPRITE_H, SPRITE_W } from "./sprites";

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

  it("accessories fit every style and pose, use known colors and show", () => {
    const look = characterLook("agent", "#d97757");
    for (const accessory of ACCESSORIES.filter((a) => a !== "none")) {
      for (const style of HAIR_STYLES) {
        for (const frame of CHARACTER_FRAMES) {
          const rows = characterRows(style, frame, accessory);
          expect(rows.length).toBe(SPRITE_H);
          for (const row of rows) {
            expect(row.length).toBe(SPRITE_W);
            for (const cell of row) if (cell !== ".") expect(look.palette[cell], `${accessory}:${style}:${frame} "${cell}"`).toBeDefined();
          }
        }
        expect(characterRows(style, "stand", accessory), `${accessory}:${style}`).not.toEqual(characterRows(style, "stand"));
      }
    }
  });

  it("a custom look overrides what the agent key would give, and only that", () => {
    const plain = characterLook("claude:s1:main", "#d97757");
    const custom = characterLook("claude:s1:main", "#d97757", false, { hair: "#b83c2e", shirt: "#4c8df5", style: "cap", accessory: "glasses" });
    expect(custom.palette.h).toBe("#b83c2e");
    expect(custom.palette.c).toBe("#4c8df5");
    expect(custom.style).toBe("cap");
    expect(custom.accessory).toBe("glasses");
    expect(custom.palette.s).toBe(plain.palette.s);
    expect(custom.id).not.toBe(plain.id);
    expect(characterLook("claude:s1:sub", "#d97757", true, { shirt: "#4c8df5" }).palette.c).toBe(shade("#4c8df5", 1.35));
  });

  it("Scale2x doubles a grid, keeps its colors and rounds diagonals", () => {
    const hd = scale2x(characterRows("short", "stand"));
    expect(hd.length).toBe(SPRITE_H * 2);
    for (const row of hd) expect(row.length).toBe(SPRITE_W * 2);
    expect(new Set(hd.join(""))).toEqual(new Set(characterRows("short", "stand").join("")));
    // A diagonal step gets its corner filled instead of a 2×2 staircase.
    expect(scale2x(["x.", ".x"])).toEqual(["xx..", "xx..", "..xx", "..xx"].map((r, i) => (i === 1 ? "xxx." : i === 2 ? ".xxx" : r)));
  });

  it("icons use only palette colors", () => {
    for (const [name, rows] of Object.entries(ICONS)) {
      for (const row of rows) for (const cell of row) if (cell !== ".") expect(ICON_PALETTE[cell], name).toBeDefined();
    }
  });
});
