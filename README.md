# Agent Office

**Agent Office** is a native Windows desktop app that shows the AI coding agents you
have running — Claude Code, OpenAI Codex CLI and Cursor CLI — as characters in a
living pixel-art office. Each session is an employee: you can see who is coding, who
is running tests, who is waiting for your permission and who just hit an error, and
you can act on them when the provider allows it.

> **Status: early development (Phase 4 of 10).** The desktop shell, unified event
> pipeline, SQLite storage, provider detection, a clearly-labelled *demo* provider,
> **Claude Code** and **Codex CLI** (sessions launched from the app, and sessions
> in your own terminal through hooks) work. Cursor arrives in Phase 5. See
> [docs/ROADMAP.md](docs/ROADMAP.md).

## What it is

* A **pixel-art office** where every agent session is a character, subagents are
  extra employees linked to their lead, and rooms reflect activity (desks, terminal
  area, QA, meeting room, lounge, CEO office).
* A **Command Center** view: active agents, waiting approvals, errors, completed
  tasks, projects, branches, tool calls and token usage.
* A **provider-agnostic platform**: every provider is an adapter that turns its
  official events (hooks, JSON-RPC protocols, stream-json) into one unified event
  model. The UI never sees provider-specific data.
* **Local-first**: no accounts, no cloud, no telemetry. Data stays in
  `%LOCALAPPDATA%\AgentOffice`.

## Supported providers

| Provider | Managed sessions (launched by the app) | External sessions (your own terminal) |
| --- | --- | --- |
| Claude Code | `claude -p` stream-json + hooks | Global hooks in `%USERPROFILE%\.claude\settings.json` (install/uninstall/repair from the app) |
| OpenAI Codex CLI | `codex app-server` (JSON-RPC) | Hooks in `%USERPROFILE%\.codex\hooks.json` (needs one-time trust via `/hooks` in Codex) |
| Cursor CLI | `agent acp` (Agent Client Protocol) | Experimental (Cursor CLI fires only some hooks) |

The exact, honest list of what each provider supports is in
[docs/PROVIDER_CAPABILITIES.md](docs/PROVIDER_CAPABILITIES.md). When a provider
does not expose something (for example token usage for external sessions), the app
shows **"Unavailable"** instead of guessing.

## Installation

End users (once Phase 10 ships): download and run **`AgentOfficeSetup.exe`**. It
installs Agent Office with a Start Menu entry, an optional desktop shortcut and an
uninstaller. Nothing else is required — no Node, Rust, Python, WSL or Bash. The
installer uses the WebView2 runtime that ships with Windows 11.

You need the agent CLIs you want to watch installed as usual (`claude`, `codex`,
`agent`). Agent Office detects them automatically.

## Watching Claude Code

1. Open **Diagnostics** and press **Install hooks** next to Claude Code (confirm).
   Agent Office adds its entries to `%USERPROFILE%\.claude\settings.json`, keeps
   everything else, and saves a backup next to the file first.
2. Run `claude` in any terminal: a character appears and follows what it does.
3. Or press **New agent** → Claude Code to launch a session from the app; it takes
   prompts from the agent panel and asks you to Approve / Reject tool use there.

Permission prompts of sessions in your own terminal are only *shown* by default. Turn
on *Answer permission requests of external sessions* in Diagnostics → Settings to
answer them from the app (Claude shows its own prompt if you don't answer in time).

## Watching Codex CLI

1. In **Diagnostics**, press **Install hooks** next to Codex CLI (confirm). Agent
   Office adds its entries at the end of `%USERPROFILE%\.codex\hooks.json`.
2. Codex asks you to review new hooks: open `codex`, type `/hooks` and trust the
   Agent Office entries. Until then Diagnostics shows *Needs your action* and
   Codex runs none of them.
3. Sessions you start with `codex` now appear in the office. **New agent** →
   Codex CLI launches a session from the app, with Approve / Reject for its
   commands and file changes.

**Before uninstalling Agent Office**, press **Uninstall** next to Claude Code and
Codex CLI in Diagnostics so they stop calling it (the installer will do this
automatically in a later phase).

## Quick start (development build)

Requirements for **development only**: Node.js 20+, Rust (stable, MSVC toolchain on
Windows), and the Tauri prerequisites for Windows (WebView2, Visual Studio Build Tools).

```powershell
npm install
npm run dev        # start the desktop app with hot reload
npm run test       # Rust + UI tests
npm run build      # Windows installer → dist-installer\AgentOfficeSetup.exe
```

`npm run dev:web` starts only the UI in a browser with a replayed demo timeline —
useful for UI work without the desktop shell.

More in [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).

## Limitations (current and by design)

* Agent Office never kills or types into a process it did not start. External
  sessions can be observed, not controlled (except Claude background sessions via the
  official `claude stop`).
* External sessions only appear if the provider's hook integration is installed (and,
  for Codex, trusted).
* Token usage and cost come only from providers that report them. Claude's cost is a
  client-side estimate reported by Claude Code and is labelled as such.
* File changes are attributed to an agent only when a tool call proves it; other
  repository changes are shown as unattributed.
* Cursor CLI support needs on-machine verification on Windows (Phase 5).
* Windows 11 x64 is the only packaged target for now; the code keeps OS-specific parts
  behind interfaces so macOS/Linux can be added later.

## Documentation

* [Architecture](docs/ARCHITECTURE.md)
* [Provider capabilities](docs/PROVIDER_CAPABILITIES.md)
* [Roadmap, risks and vertical slice](docs/ROADMAP.md)
* [Adding a provider](docs/ADDING_A_PROVIDER.md)
* [Development](docs/DEVELOPMENT.md)
* [Security](docs/SECURITY.md)

Provider names are used only to identify the tools Agent Office integrates with;
the app's characters and badges are original art and do not use provider logos.
