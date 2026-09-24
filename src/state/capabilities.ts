import type { Capabilities } from "../bindings/Capabilities";
import type { ProviderInfo } from "../bindings/ProviderInfo";
import type { SessionState } from "../bindings/SessionState";
import type { Support } from "../bindings/Support";

export type Action = "stop" | "sendPrompt" | "permissions" | "launch";

export interface ActionAvailability {
  enabled: boolean;
  /** Why the action is disabled (shown as a tooltip), or the support level badge. */
  reason: string;
  support: Support;
}

/** Capabilities for the session's mode, plus whether this build implements that mode. */
export function modeCapabilities(
  provider: ProviderInfo | undefined,
  mode: SessionState["mode"],
): { caps: Capabilities | null; implemented: boolean } {
  if (!provider) return { caps: null, implemented: false };
  const profile = provider.capabilities;
  return mode === "managed"
    ? { caps: profile.managed, implemented: profile.implemented.managedSessions }
    : { caps: profile.external, implemented: profile.implemented.externalSessions };
}

/**
 * Decides whether an action button is enabled. Actions are enabled only when
 * the provider supports the capability for this session mode AND this build
 * implements it. Nothing is ever simulated for a real provider.
 */
export function actionAvailability(
  provider: ProviderInfo | undefined,
  mode: SessionState["mode"],
  action: Action,
): ActionAvailability {
  const { caps, implemented } = modeCapabilities(provider, mode);
  if (!caps) return { enabled: false, reason: "Unknown provider", support: "unsupported" };
  const support = caps[action];
  const name = provider?.descriptor.displayName ?? "This provider";
  if (support === "unsupported") {
    return { enabled: false, reason: `${name} does not support this for ${mode} sessions`, support };
  }
  if (!implemented) {
    return { enabled: false, reason: `${name} ${mode} sessions are not implemented yet in this build`, support };
  }
  return { enabled: true, reason: support === "supported" ? "" : `Support level: ${support}`, support };
}
