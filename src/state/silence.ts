import type { Activity } from "../bindings/Activity";
import type { AgentState } from "../bindings/AgentState";
import type { SessionState } from "../bindings/SessionState";

/** Activities in which a long silence is worth a warning (same rule as the core). */
const BUSY: ReadonlySet<Activity> = new Set(["THINKING", "READING", "CODING", "RUNNING_COMMAND", "TESTING"]);

/**
 * Since when a busy agent has been silent, or null. The core flags the session
 * (`silentSince`) after the "warn when silent" preference; this only decides
 * which of its agents show the warning. Nothing is ever stopped for it.
 */
export function silentSince(
  agent: Pick<AgentState, "activity" | "ended"> | undefined,
  session: Pick<SessionState, "status" | "silentSince"> | undefined,
): number | null {
  if (!agent || !session || agent.ended || session.status !== "active") return null;
  if (session.silentSince == null || !BUSY.has(agent.activity)) return null;
  return session.silentSince;
}
