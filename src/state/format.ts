import type { Activity } from "../bindings/Activity";
import type { Support } from "../bindings/Support";
import type { UsageSnapshot } from "../bindings/UsageSnapshot";

export const ACTIVITY_LABEL: Record<Activity, string> = {
  IDLE: "Idle",
  THINKING: "Thinking",
  READING: "Reading",
  CODING: "Coding",
  RUNNING_COMMAND: "Running command",
  TESTING: "Testing",
  WAITING_PERMISSION: "Waiting for permission",
  WAITING_INPUT: "Waiting for your answer",
  ERROR: "Error",
  DONE: "Done",
};

export const ACTIVITY_COLOR: Record<Activity, string> = {
  IDLE: "#8a93a6",
  THINKING: "#b58cff",
  READING: "#5aa9ff",
  CODING: "#3bd1c6",
  RUNNING_COMMAND: "#f2c94c",
  TESTING: "#7bdc6b",
  WAITING_PERMISSION: "#ff5c5c",
  WAITING_INPUT: "#ff9f43",
  ERROR: "#ff3b3b",
  DONE: "#7bdc6b",
};

export function formatDuration(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${s % 60}s`;
  const h = Math.floor(m / 60);
  return `${h}h ${m % 60}m`;
}

export function formatTime(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

export function formatTokens(n: number | undefined): string {
  if (n === undefined) return "—";
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return String(n);
}

/** Usage text that never invents numbers: "Unavailable" when nothing was reported. */
export function formatUsage(usage: UsageSnapshot | undefined, supported: boolean): string {
  const tokens =
    usage && (usage.inputTokens !== undefined || usage.outputTokens !== undefined || usage.totalTokens !== undefined);
  if (!usage || (!tokens && usage.contextTokens === undefined)) {
    return supported ? "Not reported yet" : "Unavailable";
  }
  const parts: string[] = [];
  if (tokens) {
    parts.push(`in ${formatTokens(usage.inputTokens)}`, `out ${formatTokens(usage.outputTokens)}`);
    if (usage.cachedInputTokens !== undefined) parts.push(`cached ${formatTokens(usage.cachedInputTokens)}`);
  }
  if (usage.contextTokens !== undefined) {
    const window = usage.contextWindow !== undefined ? ` / ${formatTokens(usage.contextWindow)}` : "";
    parts.push(`context ${formatTokens(usage.contextTokens)}${window}`);
  }
  return parts.join(" · ");
}

export function formatCost(usage: UsageSnapshot | undefined): string {
  if (usage?.costUsd === undefined) return "Unavailable";
  const value = `$${usage.costUsd.toFixed(usage.costUsd < 1 ? 4 : 2)}`;
  return usage.costIsEstimate ? `${value} (estimate reported by provider)` : value;
}

export const SUPPORT_LABEL: Record<Support, string> = {
  supported: "Supported",
  partial: "Partial",
  experimental: "Experimental",
  runtime: "Negotiated at runtime",
  unsupported: "Unsupported",
};

export function isAvailable(support: Support): boolean {
  return support !== "unsupported";
}

export function shortPath(path: string | undefined, keep = 2): string {
  if (!path) return "—";
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts.length <= keep ? path : `…/${parts.slice(-keep).join("/")}`;
}
