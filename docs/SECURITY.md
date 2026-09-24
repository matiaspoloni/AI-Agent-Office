# Security

Agent Office watches tools that can read and change your source code. It is designed
to add as little attack surface as possible.

## Principles

1. **Local-first.** No accounts, no telemetry, no cloud backend. Prompts, source
   code, tool output and credentials are never sent anywhere by Agent Office.
2. **No network listeners.** The app opens no TCP/UDP port. UI ↔ core uses Tauri's
   in-process IPC. Hooks reach the app through a local **named pipe**
   (`\\.\pipe\agent-office-<user-hash>`) created with `PIPE_REJECT_REMOTE_CLIENTS`.
3. **Authenticated hook relay.** The relay presents a per-user random token stored in
   `%LOCALAPPDATA%\AgentOffice\ipc.token` (protected by the user profile ACL). Frames
   without a valid token are dropped.
4. **Least privilege in the UI.** The webview can only call the Tauri commands
   explicitly allowed in `src-tauri/capabilities/`. No shell/fs plugins are exposed to
   the renderer. A strict Content-Security-Policy blocks remote scripts.
5. **Never kill what we didn't start.** Only processes created by Agent Office (and
   tracked in their own Job Object) can be stopped or killed.
6. **Safe config edits.** Provider config files (e.g. `%USERPROFILE%\.claude\settings.json`)
   are parsed first (invalid JSON → refuse), backed up with a timestamp, written
   atomically, and only entries carrying the Agent Office marker are ever changed or
   removed. Uninstall restores the file to the user's own content.
7. **Fail open for the agent.** If the app is closed or the relay fails, the hook exits
   0 with no output, so the agent behaves as if Agent Office did not exist. Agent Office
   never auto-approves anything; approvals only happen after an explicit user click.

## Secrets

* Agent Office never reads provider credential stores or API keys.
* A redaction pass masks values that look like secrets (API keys, bearer tokens,
  `*_API_KEY=…`, `Authorization:` headers, private-key blocks) before events are
  stored or logged.
* Logs never print environment variables.

## Data at rest

* SQLite database and logs live under `%LOCALAPPDATA%\AgentOffice` (per-user).
* Retention is configurable (default 14 days of events); large tool outputs are
  truncated at ingest (default 8 KB). Storing prompt text can be disabled.

## Hooks and trust

* Claude Code user-level hooks always run; Agent Office's entry is visible in `/hooks`.
* Codex requires the user to trust hooks (`/hooks`); Agent Office never bypasses it for
  external sessions.
* Hooks from repositories (`.claude/settings.json`, `.codex/hooks.json`,
  `.cursor/hooks.json`) are never written by Agent Office.

## Reporting a vulnerability

Please open a private security advisory on the repository instead of a public issue.
