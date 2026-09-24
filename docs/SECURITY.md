# Security

Agent Office watches tools that can read and change your source code. It is designed
to add as little attack surface as possible.

## Principles

1. **Local-first.** No accounts, no telemetry, no cloud backend. Prompts, source
   code, tool output and credentials are never sent anywhere by Agent Office.
2. **No network listeners.** The app opens no TCP/UDP port. UI ↔ core uses Tauri's
   in-process IPC. Hooks reach the app through a local **named pipe**
   (`\\.\pipe\agent-office-v1-<hash>`) created as the first instance of that name (so
   no other process can squat on it) and rejecting remote clients. Development builds
   on Linux/macOS use a Unix socket with mode 0600.
3. **Authenticated hook relay.** The relay presents a per-user random token (256 bits)
   stored in `%LOCALAPPDATA%\AgentOffice\ipc.token` (protected by the user profile ACL;
   0600 on Unix). The app compares it in constant time and drops frames without it.
4. **Least privilege in the UI.** The webview can only call the Tauri commands
   explicitly allowed in `src-tauri/capabilities/`. No shell/fs plugins are exposed to
   the renderer. A strict Content-Security-Policy blocks remote scripts.
5. **Never kill what we didn't start.** Only processes created by Agent Office (and
   tracked in their own Job Object) can be stopped or killed.
6. **Safe config edits.** Provider config files (e.g. `%USERPROFILE%\.claude\settings.json`)
   are parsed first (invalid JSON → refuse), backed up with a timestamp (last 5 kept),
   written atomically (temporary file + rename), and only hook entries that run the
   Agent Office executable for that provider are ever changed or removed. Uninstall
   restores the file to the user's own content. Every change needs an explicit click
   and confirmation in Diagnostics.
7. **Fail open for the agent.** If the app is closed or the relay fails, the hook exits
   0 with no output, so the agent behaves as if Agent Office did not exist. Agent Office
   never auto-approves anything; approvals only happen after an explicit user click.
   An unanswered request is denied (managed sessions) or handed back to the agent's
   own prompt (external sessions). Answering external sessions' requests from the app
   is off by default.

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

## What hooks carry

Hook payloads can contain prompts, file paths, commands and tool output. The relay
sends them only to the local app over the pipe above; the app applies redaction and
size limits before anything is stored, and nothing leaves the machine.

Codex runs hook commands through a shell (`cmd.exe` on Windows). Agent Office
quotes its own path for that shell and refuses to install when the path holds
characters `cmd.exe` would still interpret inside quotes (`"`, `%`). Codex
hooks only run after the user trusts them in Codex (`/hooks`); Agent Office
reads that trust status (by starting `codex app-server` briefly and asking
`hooks/list`) but never writes it. Like any Codex start, that short run may let
Codex contact its own services; Agent Office sends it nothing but the request.

Managed Cursor sessions run `agent acp` in the project folder. Agent Office
declares no file-system or terminal capabilities to the agent (Cursor uses its own
tools, under its own permission checks), passes no MCP servers, and answers
Cursor's own extension requests with "not supported". Login stays Cursor's own:
Agent Office calls the `cursor_login` method, which uses the login the user made
with `agent login`, and never reads `~/.cursor` or `CURSOR_API_KEY`. Nothing is
written to Cursor's configuration.

Managed Claude sessions get a per-session settings file in
`%LOCALAPPDATA%\AgentOffice\sessions\`. It contains only the relay command and is
deleted when the session ends. Session discovery runs the official, read-only
`claude agents --json`.

## Hooks and trust

* Claude Code user-level hooks always run; Agent Office's entry is visible in `/hooks`.
* Codex requires the user to trust hooks (`/hooks`); Agent Office never bypasses it
  and never edits trust state. Its entries are appended after the user's, so
  existing hooks keep their trust.
* Hooks from repositories (`.claude/settings.json`, `.codex/hooks.json`,
  `.cursor/hooks.json`) are never written by Agent Office.

## Reporting a vulnerability

Please open a private security advisory on the repository instead of a public issue.
