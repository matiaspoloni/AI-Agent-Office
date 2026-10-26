// The attention strip shows time since the office opened, never a fixed
// two-hour ruler. Once the office has been open for two hours the visible
// span stays one millisecond under that, and older marks fall off the left.

/** Strictly under two hours. */
export const TIMELINE_MAX_MS = 2 * 60 * 60 * 1000 - 1;

export function timelineWindow(openedAt: number, now: number, latest = now): { start: number; end: number } {
  const end = Math.max(now, openedAt, latest);
  const start = Math.max(openedAt, end - TIMELINE_MAX_MS);
  return { start, end };
}

export function timelineSpan(start: number, end: number): number {
  return Math.max(end - start, 1);
}

/** 0 at the start of the open window, 100 at its end. */
export function markLeftPercent(at: number, start: number, end: number): number {
  const raw = ((at - start) / timelineSpan(start, end)) * 100;
  if (raw < 0) return 0;
  if (raw > 100) return 100;
  return raw;
}

/** Label for the visible window: how long the office has been open, capped under 2 h. */
export function timelineLabel(openedAt: number, now: number, latest = now): string {
  const { start, end } = timelineWindow(openedAt, now, latest);
  const span = end - start;
  if (span < 60_000) {
    const seconds = Math.floor(span / 1000);
    return seconds < 1 ? "Open" : `Open ${seconds} s`;
  }
  const minutes = Math.floor(span / 60_000);
  if (minutes < 60) return `Open ${minutes} min`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest === 0 ? `Open ${hours} h` : `Open ${hours} h ${rest} min`;
}
