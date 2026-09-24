// Launch options each CLI documents in its own `--help`. They are only UI
// hints: the adapter validates them again before starting a process, and a
// provider without an entry simply gets free-text fields.

export interface LaunchHints {
  /** Aliases suggested for the model field (free text is still allowed). */
  models: string[];
  /** Values for the permission mode selector; empty = no selector. */
  permissionModes: { value: string; label: string }[];
}

export const LAUNCH_HINTS: Record<string, LaunchHints> = {
  // `claude --help` (2.1.281): --model accepts an alias or a full model name;
  // --permission-mode choices are listed verbatim.
  claude: {
    models: ["fable", "opus", "sonnet"],
    permissionModes: [
      { value: "manual", label: "manual — ask before each risky action" },
      { value: "acceptEdits", label: "acceptEdits — file edits are accepted automatically" },
      { value: "plan", label: "plan — analyse and plan without changing files" },
      { value: "auto", label: "auto" },
      { value: "dontAsk", label: "dontAsk — only pre-approved tools run" },
      { value: "bypassPermissions", label: "bypassPermissions — no checks at all (dangerous)" },
    ],
  },
  // `thread/start.approvalPolicy` (AskForApproval in the app-server protocol of
  // codex-cli 0.156.1); descriptions from `codex --help` where it lists them.
  // Codex documents no model aliases, so none are suggested.
  codex: {
    models: [],
    permissionModes: [
      { value: "untrusted", label: "untrusted — ask before commands Codex does not consider safe" },
      { value: "on-request", label: "on-request — the model decides when to ask for approval" },
      { value: "never", label: "never — never ask; failures go straight back to the model" },
    ],
  },
};
