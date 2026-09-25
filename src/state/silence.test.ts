import { describe, expect, it } from "vitest";
import { silentSince } from "./silence";

describe("silentSince", () => {
  const session = { status: "active" as const, silentSince: 1000 };

  it("warns only for busy agents of a flagged, running session", () => {
    expect(silentSince({ activity: "RUNNING_COMMAND", ended: false }, session)).toBe(1000);
    expect(silentSince({ activity: "THINKING", ended: false }, session)).toBe(1000);
    // Waiting on the user or idle is not "stuck".
    expect(silentSince({ activity: "WAITING_PERMISSION", ended: false }, session)).toBeNull();
    expect(silentSince({ activity: "IDLE", ended: false }, session)).toBeNull();
    expect(silentSince({ activity: "CODING", ended: true }, session)).toBeNull();
    expect(silentSince({ activity: "CODING", ended: false }, { status: "ended", silentSince: 1000 })).toBeNull();
    expect(silentSince({ activity: "CODING", ended: false }, { status: "active" })).toBeNull();
    expect(silentSince(undefined, session)).toBeNull();
  });
});
