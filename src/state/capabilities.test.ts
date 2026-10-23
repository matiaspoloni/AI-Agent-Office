import { describe, expect, it } from "vitest";
import type { ProviderInfo } from "../bindings/ProviderInfo";
import type { SessionState } from "../bindings/SessionState";
import type { UiBatch } from "../bindings/UiBatch";
import recording from "../ipc/preview-recording.json";
import { continueAvailability, promptAvailability } from "./capabilities";

const data = recording as unknown as { providers: ProviderInfo[]; frames: { batch: UiBatch }[] };

/** A provider as this build ships it: both session kinds implemented. */
function provider(id: string): ProviderInfo {
  const base = structuredClone(data.providers.find((p) => p.descriptor.id === id)!);
  base.capabilities.implemented.managedSessions = true;
  base.capabilities.implemented.externalSessions = true;
  return base;
}

const sample = data.frames.flatMap((f) => f.batch.sessions)[0];
function session(over: Partial<SessionState>): SessionState {
  return { ...structuredClone(sample), cwd: "/work/app", ...over };
}

describe("instructions from the office", () => {
  const claude = provider("claude");

  it("go to sessions started from Agent Office while they run", () => {
    expect(promptAvailability(claude, session({ mode: "managed", status: "active" })).enabled).toBe(true);
    const ended = promptAvailability(claude, session({ mode: "managed", status: "ended" }));
    expect(ended.enabled).toBe(false);
    expect(ended.reason).toMatch(/Restart/);
  });

  it("never go into the user's terminal, and say how to continue instead", () => {
    const open = promptAvailability(claude, session({ mode: "external", status: "active" }));
    expect(open.enabled).toBe(false);
    expect(open.reason).toMatch(/cannot type into it/);
    const ended = promptAvailability(claude, session({ mode: "external", status: "ended" }));
    expect(ended.enabled).toBe(false);
    expect(ended.reason).toMatch(/Continue it in Agent Office/);
  });
});

describe("continue in Agent Office", () => {
  const claude = provider("claude");

  it("only once the terminal session has ended", () => {
    const open = continueAvailability(claude, session({ mode: "external", status: "active" }));
    expect(open.enabled).toBe(false);
    expect(open.reason).toMatch(/Close it there/);
    expect(continueAvailability(claude, session({ mode: "external", status: "ended" })).enabled).toBe(true);
  });

  it("needs the session's folder and a provider that can resume", () => {
    expect(continueAvailability(claude, session({ mode: "external", status: "ended", cwd: undefined })).enabled).toBe(
      false,
    );
    const noResume = provider("claude");
    noResume.capabilities.managed.resume = "unsupported";
    const refused = continueAvailability(noResume, session({ mode: "external", status: "ended" }));
    expect(refused.enabled).toBe(false);
    expect(refused.reason).toMatch(/cannot be continued/);
  });

  it("is Restart for sessions started from Agent Office", () => {
    const managed = continueAvailability(claude, session({ mode: "managed", status: "ended" }));
    expect(managed.enabled).toBe(false);
    expect(managed.reason).toMatch(/Restart/);
  });
});
