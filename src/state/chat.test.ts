import { describe, expect, it } from "vitest";
import type { AgentEvent } from "../bindings/AgentEvent";
import { attachPending, conversationFromEvents, type PendingPrompt } from "./chat";

function ev(over: Pick<AgentEvent, "type" | "payload"> & Partial<AgentEvent>): AgentEvent {
  return {
    eventId: over.eventId ?? `${over.type}-${over.timestamp ?? 1}`,
    timestamp: over.timestamp ?? 1,
    provider: "demo",
    sessionId: "s",
    agentId: "s",
    source: "simulation",
    ...over,
  };
}

const focus = { focusAgentId: "s", names: { s: "Nalu", sub: "Explorer" } };

describe("conversationFromEvents", () => {
  it("shows the prompt and the full reply as a conversation", () => {
    const items = conversationFromEvents(
      [
        ev({
          eventId: "p",
          timestamp: 1,
          type: "prompt.submitted",
          payload: { text: "Add CSV export" },
        }),
        ev({
          eventId: "t",
          timestamp: 2,
          type: "tool.started",
          payload: { toolName: "Read", category: "read", title: "Read reports.ts" },
        }),
        ev({
          eventId: "m",
          timestamp: 3,
          type: "agent.message",
          payload: { text: "Export is on the reports page.\nIt writes reports.csv." },
        }),
      ],
      focus,
    );
    expect(items.map((i) => i.role)).toEqual(["user", "note", "agent"]);
    expect(items[0].text).toBe("Add CSV export");
    expect(items[1].text).toBe("Read reports.ts");
    expect(items[2].text).toBe("Export is on the reports page.\nIt writes reports.csv.");
    expect(items[2].speaker).toBeUndefined();
  });

  it("joins a reply the provider sends again, longer, into one bubble", () => {
    const items = conversationFromEvents(
      [
        ev({ eventId: "a", timestamp: 1, type: "agent.message", payload: { text: "Hello" } }),
        ev({ eventId: "b", timestamp: 2, type: "agent.message", payload: { text: "Hello world" } }),
      ],
      focus,
    );
    expect(items).toHaveLength(1);
    expect(items[0].text).toBe("Hello world");
    expect(items[0].id).toBe("a");
  });

  it("keeps two different replies, and names a subagent", () => {
    const items = conversationFromEvents(
      [
        ev({ eventId: "a", timestamp: 1, type: "agent.message", payload: { text: "Looking" } }),
        ev({
          eventId: "b",
          timestamp: 2,
          type: "agent.message",
          agentId: "sub",
          payload: { text: "Found three failures" },
        }),
      ],
      focus,
    );
    expect(items.map((i) => i.text)).toEqual(["Looking", "Found three failures"]);
    expect(items[1].speaker).toBe("Explorer");
  });

  it("collapses a run of tool calls and keeps a failure visible", () => {
    const items = conversationFromEvents(
      [
        ev({ eventId: "1", timestamp: 1, type: "tool.started", payload: { toolName: "Read", category: "read", title: "Read a.ts" } }),
        ev({ eventId: "2", timestamp: 2, type: "tool.started", payload: { toolName: "Read", category: "read", title: "Read b.ts" } }),
        ev({ eventId: "3", timestamp: 3, type: "tool.started", payload: { toolName: "Edit", category: "edit", title: "Edit c.ts" } }),
        ev({ eventId: "4", timestamp: 4, type: "tool.started", payload: { toolName: "Bash", category: "execute", title: "npm test" } }),
        ev({
          eventId: "5",
          timestamp: 5,
          type: "tool.failed",
          payload: { toolName: "Bash", category: "execute", detail: "exit 1" },
        }),
      ],
      focus,
    );
    expect(items).toHaveLength(1);
    expect(items[0].text).toBe("5 actions · last: Failed: Bash — exit 1");
    expect(items[0].tone).toBe("bad");
  });

  it("leaves file reads, usage and thinking out of the conversation", () => {
    const items = conversationFromEvents(
      [
        ev({ eventId: "1", timestamp: 1, type: "agent.thinking", payload: { text: "hmm" } }),
        ev({ eventId: "2", timestamp: 2, type: "file.read", payload: { path: "a.ts" } }),
        ev({ eventId: "3", timestamp: 3, type: "usage.updated", payload: { costIsEstimate: false, inputTokens: 10 } }),
      ],
      focus,
    );
    expect(items).toEqual([]);
  });

  it("says when a prompt was stored without its text", () => {
    const items = conversationFromEvents([ev({ type: "prompt.submitted", payload: {} })], focus);
    expect(items[0].text).toBe("Prompt sent (text not stored)");
  });
});

describe("attachPending", () => {
  const pending: PendingPrompt = { id: "local", text: "Add CSV export", at: 10_000 };

  it("shows the typed prompt until the event with the same text arrives", () => {
    const before = attachPending([], [pending], []);
    expect(before.map((i) => i.text)).toEqual(["Add CSV export"]);
    const landed = ev({
      eventId: "p",
      timestamp: 10_100,
      type: "prompt.submitted",
      payload: { text: "Add CSV export" },
    });
    const after = attachPending(conversationFromEvents([landed], focus), [pending], [landed]);
    expect(after.filter((i) => i.role === "user")).toHaveLength(1);
    expect(after[0].id).toBe("p");
  });

  it("keeps the typed text when the event stores none", () => {
    const landed = ev({ eventId: "p", timestamp: 10_100, type: "prompt.submitted", payload: {} });
    const items = attachPending(conversationFromEvents([landed], focus), [pending], [landed]);
    expect(items.map((i) => i.text)).toEqual(["Add CSV export"]);
    expect(items[0].id).toBe("local");
  });
});
