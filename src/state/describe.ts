import type { AgentEvent } from "../bindings/AgentEvent";

/** One-line, human readable summary of a normalized event (for logs). */
export function describeEvent(e: AgentEvent): string {
  switch (e.type) {
    case "session.started":
      return `Session started${e.payload.model ? ` · ${e.payload.model}` : ""}${e.payload.cwd ? ` · ${e.payload.cwd}` : ""}`;
    case "session.updated":
      return "Session updated";
    case "session.ended":
      return `Session ended${e.payload.reason ? ` (${e.payload.reason})` : ""}`;
    case "prompt.submitted":
      return e.payload.text ? `Prompt: ${e.payload.text}` : "Prompt submitted";
    case "agent.thinking":
      return e.payload.text ? `Thinking: ${e.payload.text}` : "Thinking";
    case "agent.message":
      return e.payload.text ?? "Message";
    case "agent.idle":
      return e.payload.text ?? "Idle";
    case "agent.waiting":
      return e.payload.message ?? `Waiting (${e.payload.reason})`;
    case "agent.error":
      return `Error: ${e.payload.message}`;
    case "tool.started":
      return `▶ ${e.payload.title ?? e.payload.toolName}`;
    case "tool.completed":
      return `✔ ${e.payload.toolName}${e.payload.durationMs !== undefined ? ` (${e.payload.durationMs} ms)` : ""}`;
    case "tool.failed":
      return `✖ ${e.payload.toolName}${e.payload.detail ? `: ${e.payload.detail}` : ""}`;
    case "file.read":
      return `Read ${e.payload.path}`;
    case "file.created":
      return `Created ${e.payload.path}`;
    case "file.modified":
      return `Modified ${e.payload.path}`;
    case "file.deleted":
      return `Deleted ${e.payload.path}`;
    case "command.started":
      return `$ ${e.payload.command}`;
    case "command.output":
      return `  ${e.payload.chunk}`;
    case "command.completed":
      return `Command finished (exit ${e.payload.exitCode ?? "?"})`;
    case "command.failed":
      return `Command failed (exit ${e.payload.exitCode ?? "?"})`;
    case "permission.requested":
      return `Permission requested: ${e.payload.description}`;
    case "permission.approved":
      return `Permission approved (${e.payload.resolvedBy})`;
    case "permission.denied":
      return `Permission denied (${e.payload.resolvedBy})${e.payload.message ? `: ${e.payload.message}` : ""}`;
    case "permission.expired":
      return e.payload.message ?? "No answer in Agent Office; answer in the agent's own prompt";
    case "subagent.started":
      return `Subagent started: ${e.payload.agentType ?? "subagent"}${e.payload.description ? ` — ${e.payload.description}` : ""}`;
    case "subagent.updated":
      return `Subagent update${e.payload.description ? `: ${e.payload.description}` : ""}`;
    case "subagent.ended":
      return "Subagent finished";
    case "git.branch_changed":
      return `Branch: ${e.payload.branch ?? "detached"}`;
    case "git.commit_created":
      return `Commit ${e.payload.sha.slice(0, 7)}: ${e.payload.summary}`;
    case "git.status_changed":
      return `Git: ${e.payload.dirty} dirty, ${e.payload.staged} staged`;
    case "context.compacted":
      return "Context compacted";
    case "usage.updated":
      return `Usage: in ${e.payload.inputTokens ?? "?"} / out ${e.payload.outputTokens ?? "?"}`;
    case "provider.error":
      return `Provider error (${e.payload.component}): ${e.payload.message}`;
  }
}

export function eventTone(e: AgentEvent): "normal" | "good" | "bad" | "warn" | "muted" {
  switch (e.type) {
    case "agent.error":
    case "tool.failed":
    case "command.failed":
    case "provider.error":
    case "permission.denied":
      return "bad";
    case "permission.requested":
    case "permission.expired":
      return "warn";
    case "tool.completed":
    case "command.completed":
    case "permission.approved":
      return "good";
    case "command.output":
    case "file.read":
      return "muted";
    default:
      return "normal";
  }
}
