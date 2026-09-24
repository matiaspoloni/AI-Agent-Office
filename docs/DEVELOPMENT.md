# Development

Everything here is for **developers**. End users only need `AgentOfficeSetup.exe`.

## Prerequisites (Windows 11)

1. **Node.js 20+** (22 LTS recommended) — <https://nodejs.org>
2. **Rust (stable, MSVC)** via rustup — <https://rustup.rs>
3. **Visual Studio Build Tools** with the *Desktop development with C++* workload
   (MSVC linker + Windows SDK).
4. **WebView2** — already part of Windows 11.

No WSL, Bash, Make or Python is needed. All commands below work in PowerShell and
Windows Terminal.

Linux/macOS work for development too (see the Tauri prerequisites for your OS;
on Ubuntu: `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev libxdo-dev`).

## Commands

| Command | What it does |
| --- | --- |
| `npm install` | Install UI + Tauri CLI dependencies |
| `npm run dev` | Desktop app with hot reload (Vite + `tauri dev`) |
| `npm run dev:web` | UI only, in a browser, replaying a recorded **simulated** demo |
| `npm run build` | Production build + NSIS installer, copied to `dist-installer/AgentOfficeSetup.exe` |
| `npm run build:ui` | Typecheck + bundle the UI into `dist/` |
| `npm run test` | All Rust tests (`cargo test --workspace`) + UI tests (Vitest) |
| `npm run lint` | TypeScript typecheck, `cargo fmt --check`, `cargo clippy -D warnings` |
| `npm run smoke` | Headless smoke test of the desktop binary (`--smoke-test`); `SMOKE_KEEP_REPORT=<file>` keeps the report |
| `npm run bindings` | Regenerate TypeScript types from Rust (`src/bindings/`) |
| `npm run preview:record` | Regenerate the browser-preview timeline from the Rust demo provider |

## Repository layout

```
src/                  React UI (TypeScript)
  bindings/           generated from Rust with ts-rs — do not edit by hand
  ipc/                Tauri backend, browser-preview backend, recorded timeline
  state/              normalized store, formatting, capability gating
  office/             layout data, scene logic (seats, paths), canvas renderer
  views/, components/ Command Center, Projects, Diagnostics, Agent panel, dialogs
src-tauri/            desktop app: host runtime, Tauri commands, diagnostics, logging
crates/ao-core        event model, capabilities, adapter trait, pipeline, reducer, batching
crates/ao-store       SQLite schema/migrations, batched writer
crates/ao-detect      executable discovery + version probing
crates/ao-process     process trees of managed agents (Job Objects / process groups)
crates/ao-ipc         local IPC between the hook relay and the app (named pipe / Unix socket)
crates/ao-hook-relay  `agent-office hook <provider>` relay + standalone `agent-office-hook` binary
crates/ao-config      safe edits of provider config files (parse-or-refuse, backups, atomic write)
crates/ao-testkit     fixture harness, fake CLIs (`fake-claude`, `fake-codex`, `ao-fake-child`), cargo_bin
crates/providers/*    one crate per provider (claude, codex, cursor, demo)
fixtures/             provider payloads used by mapping tests (`-real` = captured from a real CLI)
scripts/              cross-platform Node scripts (smoke test, installer collection)
docs/                 architecture, capabilities, roadmap, security, this file
```

## How data flows

1. A provider adapter pushes `AgentEvent`s into an `EventSink`.
2. The host pipeline de-duplicates, redacts secrets, truncates large strings and
   resolves the project from the working folder.
3. The pure reducer (`ao_core::world`) updates `SessionState`/`AgentState`.
4. Every 100 ms the batcher sends only changed objects to the UI through a Tauri
   Channel, and the writer thread persists events + state to SQLite.

The UI never re-derives state from events; it merges the objects it receives.

## TypeScript bindings

Rust types marked `#[ts(export)]` are written to `src/bindings/` when their crate's
tests run (`TS_RS_EXPORT_DIR` is set in `.cargo/config.toml`). After changing a
shared type run `npm run bindings` (or `npm run test`) and commit the result. CI
fails if the committed bindings are stale.

## Demo provider and browser preview

The **Demo** provider (`crates/providers/ao-provider-demo`) produces clearly
labelled simulated sessions (`provider = "demo"`, `source = "simulation"`,
badge `SIM`). It exists so the office can be developed and tested without real
accounts. In the desktop app press **Demo office**.

`npm run dev:web` replays `src/ipc/preview-recording.json`, recorded by the same
Rust code on a virtual clock (`npm run preview:record`). The preview cannot
control agents or run diagnostics.

## Data, logs, overrides

| What | Where |
| --- | --- |
| Database | `%LOCALAPPDATA%\AgentOffice\agent-office.db` |
| Logs | `%LOCALAPPDATA%\AgentOffice\logs\app.log.*`, `providers.log.*` |
| Override data folder | `AGENT_OFFICE_DATA_DIR=<folder>` (hooks installed from such an instance pass `--data-dir`) |
| IPC token | `%LOCALAPPDATA%\AgentOffice\ipc.token` |
| Managed session hook files | `%LOCALAPPDATA%\AgentOffice\sessions\` (deleted when the session ends) |
| Log level | `AGENT_OFFICE_LOG=debug` (or `info,provider=debug`) |

## Tests

* **Rust unit/integration tests** live next to the code (`cargo test -p <crate>`).
  They cover the event model, pipeline, reducer, redaction, SQLite store and
  writer, detection, capability declarations, the demo provider (permissions,
  stop), preview recording, and the host runtime (event → state → UI → disk,
  restart behaviour).
* **Process and IPC tests** start real helper binaries built on demand by
  `ao_testkit::bins::cargo_bin` (process-tree kill, relay ↔ server round trips,
  bad tokens, app not running).
* **Mapping fixtures**: every file in `fixtures/claude/hooks` and
  `fixtures/claude/stream` is an input plus the expected unified events
  (`crates/providers/ao-provider-claude/tests/fixtures.rs`). Add a fixture for every
  new payload shape you see in Diagnostics → *Hook bridge*. Codex fixtures live in
  `fixtures/codex/hooks` (one payload each) and `fixtures/codex/appserver` (a
  *sequence* of app-server messages fed through the stateful mapper, approval
  requests included).
* **End-to-end** (`src-tauri/tests/claude_e2e.rs`): the real host, IPC server and
  relay driven by `fake-claude`, a stand-in for the `claude` CLI that runs configured
  hooks exactly like Claude Code (exec form, JSON on stdin, async/sync). It covers a
  managed session (prompt, approve, reject, usage, stop) and the global integration
  (install over existing settings, answering an external permission, observe-only
  mode, uninstall restoring the file). The user's real Claude configuration is
  never used: tests point the adapter at a temporary config folder.
  `src-tauri/tests/codex_e2e.rs` does the same for Codex with `fake-codex`, which
  speaks the app-server protocol and runs hooks through the shell like Codex
  (`cmd.exe /C` on Windows), only once they are "trusted" (a `fake-trust-all`
  file in its `CODEX_HOME` stands in for `/hooks`).

### Re-recording provider fixtures

The `*-real.json` fixtures come from real CLIs run against throw-away
configuration folders, never a personal account:

* **Codex**: a temporary `CODEX_HOME` whose `config.toml` defines a model
  provider with `base_url = "http://127.0.0.1:<port>/v1"`, `wire_api =
  "responses"`, pointing at a small local server that streams scripted
  Responses API events (an `exec_command` call, an `apply_patch` via
  `exec_command`, a namespaced `multi_agent_v1.spawn_agent` call, then a
  message). `codex app-server` is driven over stdio and a hook handler that
  appends its stdin to a file records the hook payloads. Nothing reaches
  OpenAI and no account is needed.
* **Claude Code**: a temporary `CLAUDE_CONFIG_DIR`. Hook payloads and
  `stream-json` shapes can be captured this way; a real prompt needs an
  authenticated CLI and **costs tokens**, so it is never part of the tests.

When a new CLI version changes a shape, re-record, update the fixture and the
mapping together, and note the finding in PROVIDER_CAPABILITIES.md.
* **UI tests** (Vitest): scene/seat allocation incl. 20 sessions + 50 subagents,
  path finding, store merging, timestamp shifting, capability gating.
* **Smoke test**: `npm run smoke` runs the real binary headless with a temporary
  data folder; CI runs it on `windows-latest`.
* Provider tests must never need real accounts: use fixtures and fake binaries
  (see [ADDING_A_PROVIDER.md](ADDING_A_PROVIDER.md)).

## Troubleshooting

* **`tauri dev` cannot find the dev server** — port 1420 must be free.
* **Blank window on Windows** — make sure WebView2 is installed/updated.
* **Database error in Diagnostics** — the app falls back to temporary in-memory
  storage and shows the error; check the path and file permissions.
* **Hook bridge "Not listening"** — another Agent Office instance with the same data
  folder already owns the pipe; close it, or start this one with a different
  `AGENT_OFFICE_DATA_DIR`.
* **No events from Claude** — Diagnostics → Providers must show *Installed* for the
  hooks; inside Claude, `/hooks` lists them. Hooks also need the folder to be trusted
  and `disableAllHooks` to be off.
