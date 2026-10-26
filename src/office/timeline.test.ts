// @vitest-environment node
import { describe, expect, it } from "vitest";
import { markLeftPercent, TIMELINE_MAX_MS, timelineLabel, timelineSpan, timelineWindow } from "./timeline";

const HOUR = 60 * 60 * 1000;

describe("office timeline window", () => {
  it("uses the time since the office opened when that is under two hours", () => {
    const openedAt = 1_000_000;
    const now = openedAt + 18 * 60 * 1000;
    const { start, end } = timelineWindow(openedAt, now);
    expect(start).toBe(openedAt);
    expect(end).toBe(now);
    expect(end - start).toBeLessThan(2 * HOUR);
    expect(timelineLabel(openedAt, now)).toBe("Open 18 min");
  });

  it("keeps the visible span under two hours after the office has been open longer", () => {
    const openedAt = 0;
    const now = 5 * HOUR;
    const { start, end } = timelineWindow(openedAt, now);
    expect(end).toBe(now);
    expect(start).toBe(now - TIMELINE_MAX_MS);
    expect(end - start).toBe(TIMELINE_MAX_MS);
    expect(end - start).toBeLessThan(2 * HOUR);
    expect(timelineLabel(openedAt, now)).toBe("Open 1 h 59 min");
  });

  it("names the first minute in seconds and an exact hour without leftover minutes", () => {
    const openedAt = 0;
    expect(timelineLabel(openedAt, 0)).toBe("Open");
    expect(timelineLabel(openedAt, 59_000)).toBe("Open 59 s");
    expect(timelineLabel(openedAt, 60_000)).toBe("Open 1 min");
    expect(timelineLabel(openedAt, HOUR)).toBe("Open 1 h");
    expect(timelineLabel(openedAt, HOUR + 4 * 60 * 1000)).toBe("Open 1 h 4 min");
  });

  it("places a mark by its position in the open window", () => {
    const openedAt = 1_000;
    const now = openedAt + 100_000;
    const { start, end } = timelineWindow(openedAt, now);
    expect(markLeftPercent(start, start, end)).toBe(0);
    expect(markLeftPercent(end, start, end)).toBe(100);
    expect(markLeftPercent(start + 25_000, start, end)).toBe(25);
    expect(markLeftPercent(start - 50, start, end)).toBe(0);
    expect(timelineSpan(now, now)).toBe(1);
  });
});
