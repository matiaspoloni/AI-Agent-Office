import type { UiBatch } from "../bindings/UiBatch";

/** Returns a copy of `batch` with every timestamp moved by `deltaMs`. */
export function shiftBatch(batch: UiBatch, deltaMs: number): UiBatch {
  const shift = (value: number | undefined) => (value === undefined ? undefined : value + deltaMs);
  return {
    ...batch,
    sessions: batch.sessions.map((s) => ({
      ...s,
      startedAt: s.startedAt + deltaMs,
      lastEventAt: s.lastEventAt + deltaMs,
      endedAt: shift(s.endedAt),
    })),
    agents: batch.agents.map((a) => ({
      ...a,
      activitySince: a.activitySince + deltaMs,
      endedAt: shift(a.endedAt),
      runningTools: a.runningTools.map((t) => ({ ...t, startedAt: t.startedAt + deltaMs })),
    })),
    events: batch.events.map((e) => ({ ...e, timestamp: e.timestamp + deltaMs })),
  };
}
