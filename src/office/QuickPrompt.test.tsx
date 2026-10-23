import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentState } from "../bindings/AgentState";
import type { ProviderInfo } from "../bindings/ProviderInfo";
import type { SessionState } from "../bindings/SessionState";
import type { UiBatch } from "../bindings/UiBatch";
import type { Backend } from "../ipc/backend";
import { BackendContext } from "../ipc/BackendContext";
import recording from "../ipc/preview-recording.json";
import { useOfficeStore } from "../state/store";
import { QuickPrompt } from "./QuickPrompt";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const data = recording as unknown as { providers: ProviderInfo[]; frames: { batch: UiBatch }[] };
const claude = structuredClone(data.providers.find((p) => p.descriptor.id === "claude")!);
claude.capabilities.implemented.managedSessions = true;
claude.capabilities.implemented.externalSessions = true;
const sampleSession = data.frames.flatMap((f) => f.batch.sessions)[0];
const sampleAgent = data.frames.flatMap((f) => f.batch.agents)[0];

function seed(sessionOver: Partial<SessionState>) {
  const session: SessionState = {
    ...structuredClone(sampleSession),
    key: "claude:s1",
    provider: "claude",
    sessionId: "s1",
    mainAgentKey: "claude:s1:main",
    cwd: "/work/app",
    ...sessionOver,
  };
  const lead: AgentState = {
    ...structuredClone(sampleAgent),
    key: "claude:s1:main",
    sessionKey: "claude:s1",
    provider: "claude",
    sessionId: "s1",
    isMain: true,
    name: "Nalu",
  };
  const sub: AgentState = { ...structuredClone(lead), key: "claude:s1:sub", isMain: false, name: "Explorer" };
  useOfficeStore.setState({
    providers: { claude },
    sessions: { [session.key]: session },
    agents: { [lead.key]: lead, [sub.key]: sub },
    toasts: [],
  });
}

let container: HTMLDivElement;
let root: Root;
const backend = {
  kind: "desktop",
  sendPrompt: vi.fn(async () => {}),
  continueSession: vi.fn(async () => ({ provider: "claude", sessionId: "s1", mode: "managed" })),
} as unknown as Backend & { sendPrompt: ReturnType<typeof vi.fn>; continueSession: ReturnType<typeof vi.fn> };

async function render(agentKey: string, onClose = vi.fn()) {
  await act(async () => {
    root.render(
      <BackendContext.Provider value={backend}>
        <QuickPrompt agentKey={agentKey} onClose={onClose} />
      </BackendContext.Provider>,
    );
  });
  return onClose;
}

async function type(text: string) {
  const box = container.querySelector("textarea")!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(box, text);
    box.dispatchEvent(new Event("input", { bubbles: true }));
  });
  return box;
}

beforeEach(() => {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  backend.sendPrompt.mockClear();
  backend.continueSession.mockClear();
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("instruction bubble", () => {
  it("sends with Enter to the session and closes", async () => {
    seed({ mode: "managed", status: "active" });
    const onClose = await render("claude:s1:main");
    expect(container.textContent).toContain("Tell Nalu");
    const box = await type("Run the tests");
    await act(async () => {
      box.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    });
    expect(backend.sendPrompt).toHaveBeenCalledWith("claude", "s1", "Run the tests");
    expect(onClose).toHaveBeenCalled();
    expect(useOfficeStore.getState().toasts.at(-1)?.text).toBe("Sent to Nalu.");
  });

  it("a subagent's bubble talks to its lead", async () => {
    seed({ mode: "managed", status: "active" });
    await render("claude:s1:sub");
    expect(container.textContent).toContain("Tell Nalu");
    expect(container.textContent).toContain("(lead of Explorer)");
    const box = await type("Summarize");
    await act(async () => {
      box.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    });
    expect(backend.sendPrompt).toHaveBeenCalledWith("claude", "s1", "Summarize");
  });

  it("Shift+Enter is a new line, Escape closes", async () => {
    seed({ mode: "managed", status: "active" });
    const onClose = await render("claude:s1:main");
    const box = await type("first line");
    await act(async () => {
      box.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", shiftKey: true, bubbles: true }));
    });
    expect(backend.sendPrompt).not.toHaveBeenCalled();
    await act(async () => {
      box.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
    expect(onClose).toHaveBeenCalled();
  });

  it("a terminal session that ended can be continued here", async () => {
    seed({ mode: "external", status: "ended" });
    await render("claude:s1:main");
    expect(container.querySelector("textarea")).toBeNull();
    const button = [...container.querySelectorAll("button")].find((b) => b.textContent === "Continue in Agent Office")!;
    expect(button.disabled).toBe(false);
    await act(async () => {
      button.click();
    });
    expect(backend.continueSession).toHaveBeenCalledWith("claude", "s1");
  });

  it("a terminal session still open says so and offers nothing to type into", async () => {
    seed({ mode: "external", status: "active" });
    await render("claude:s1:main");
    expect(container.querySelector("textarea")).toBeNull();
    expect(container.textContent).toContain("cannot type into it");
    const button = [...container.querySelectorAll("button")].find((b) => b.textContent === "Continue in Agent Office")!;
    expect(button.disabled).toBe(true);
  });
});
