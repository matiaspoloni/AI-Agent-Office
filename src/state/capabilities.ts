import type { Capabilities } from "../bindings/Capabilities";
import type { ProviderInfo } from "../bindings/ProviderInfo";
import type { SessionState } from "../bindings/SessionState";
import type { Support } from "../bindings/Support";

export type Action = "stop" | "sendPrompt" | "permissions" | "launch" | "resume";

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

export interface Availability {
  enabled: boolean;
  reason: string;
}

/**
 * Whether the app can send this session a prompt, and if not, what the user
 * can do about it. Sessions from the user's terminal never take prompts from
 * the app (no provider offers that); once ended there, they can be continued
 * in Agent Office.
 */
export function promptAvailability(provider: ProviderInfo | undefined, session: SessionState): Availability {
  if (session.mode === "external") {
    return session.status === "ended"
      ? {
          enabled: false,
          reason: "This session ended in your terminal. Continue it in Agent Office to send it prompts.",
        }
      : {
          enabled: false,
          reason:
            "Started in your terminal: Agent Office cannot type into it. Close it there, then continue it in Agent Office.",
        };
  }
  if (session.status === "ended") {
    return { enabled: false, reason: "This session has ended. Restart it to continue the conversation." };
  }
  const send = actionAvailability(provider, session.mode, "sendPrompt");
  return { enabled: send.enabled, reason: send.reason };
}

/**
 * "Continue in Agent Office" for a session from the user's terminal: possible
 * once it has ended there, when the provider can resume a conversation in a
 * session Agent Office starts (Claude `--resume`, Codex `thread/resume`).
 */
export function continueAvailability(provider: ProviderInfo | undefined, session: SessionState): Availability {
  if (session.mode !== "external") {
    return { enabled: false, reason: "Started from Agent Office: use Restart" };
  }
  const resume = actionAvailability(provider, "managed", "resume");
  if (!resume.enabled) {
    const name = provider?.descriptor.displayName ?? "This provider";
    return { enabled: false, reason: `${name} sessions from the terminal cannot be continued in Agent Office` };
  }
  if (session.status !== "ended") {
    return {
      enabled: false,
      reason: "Still open in your terminal. Close it there (for example with /exit), then continue it here.",
    };
  }
  if (!session.cwd) return { enabled: false, reason: "This session did not report its folder" };
  return {
    enabled: true,
    reason: "Continues this conversation in Agent Office, in the same folder, where you can send it prompts",
  };
}
