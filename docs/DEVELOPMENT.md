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
| `npm run bench:office` | Frame cost of the office with 20 sessions + 50 subagents (needs Playwright; see *Office*) |
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
crates/ao-jsonrpc     JSON-RPC 2.0 over a managed process's stdio (Codex app-server, ACP agents)
crates/ao-testkit     fixture harness, fake CLIs (`fake-claude`, `fake-codex`, `fake-cursor`, `ao-fake-child`), cargo_bin
crates/providers/*    one crate per provider (claude, codex, cursor, demo)
fixtures/             provider payloads used by mapping tests (`-real` = captured from a real CLI,
                      `-sdk` = the official ACP example agent, `-schema` = written from a schema)
scripts/              cross-platform Node scripts (smoke test, office benchmark, installer collection)
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

## Office (src/office)

| File | Role |
| --- | --- |
| `layout.ts` | Rooms, furniture, seats and desk pods as data; `buildGrid`, `findPath`, `validateLayout` |
| `scene.ts` | Where every agent should be: seats, project pods, settle delay, walking, exit (no drawing, unit-tested) |
| `sprites.ts` | Pixel characters composed from hair style + pose + legs; icons; cached canvases |
| `renderer.ts` | Canvas 2D drawing: floor layer, y-sorted furniture and characters, bubbles, screens, tags, plates |
| `teams.ts` | Which project team an agent belongs to (project, else folder) |
| `OfficeView.tsx` | Frame loop (60 fps while walking, 30 fps when calm), hover card, keyboard, legend |

To add a pose, add its torso/legs/patch to `POSES` in `sprites.ts` and pick it in
`frameFor` (renderer); `sprites.test.ts` checks every frame's size and colors.
New furniture is a `FurnitureKind` plus a case in `drawFurniture`;
`validateLayout` must stay empty for the default layout (tested).

**Stress test and benchmark.** `npm run dev:web`, then open `/?stress`: 20
simulated sessions with 50 subagents that keep changing activity (demo
provider, labelled SIM). To measure the frame cost:

```powershell
npm run build:ui
npm run bench:office                                      # 1600 × 1000, 10 s
npm run bench:office -- --width 1920 --height 1080 --dpr 2
```

It needs Playwright with Chromium (`npm i -g playwright`, then
`npx playwright install chromium`, or `PLAYWRIGHT_MODULE=<path to playwright/index.mjs>`).
It prints the frames drawn per second and the cost per frame; a frame fits a
60 fps display when the 95th percentile stays well under 16.7 ms.

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
  `ao_testkit::bins::cargo_bin` (process-tree kill, processes a finished agent
  left running, exit descriptions, the process list, relay ↔ server round trips,
  bad tokens, app not running). On Windows CI a real `.cmd` shim is started to
  check arguments, exit codes and that its whole tree dies with it.
* **Git tests** need `git` on PATH (like the app). `crates/ao-git/tests/repos.rs`
  builds real temporary repositories (changes of every kind, no commits yet,
  upstream ahead/behind, linked worktrees, conflicts, detached HEAD) and shows
  that Agent Office's reads do not start a configured `core.fsmonitor` program
  and do not rewrite the index — with controls proving that a plain
  `git status` does both. `src-tauri/tests/git_e2e.rs` drives the real host:
  branch/status/worktree events, files linked to the agent that wrote them,
  commits credited only with evidence (and not when two agents commit at once),
  project folders, and a missing Git.
* **Mapping fixtures**: every file in `fixtures/claude/hooks` and
  `fixtures/claude/stream` is an input plus the expected unified events
  (`crates/providers/ao-provider-claude/tests/fixtures.rs`). Add a fixture for every
  new payload shape you see in Diagnostics → *Hook bridge*. Codex fixtures live in
  `fixtures/codex/hooks` (one payload each) and `fixtures/codex/appserver` (a
  *sequence* of app-server messages fed through the stateful mapper, approval
  requests included). Cursor fixtures (`fixtures/cursor/acp`) are ACP message
  sequences with two test steps: `answered` (Agent Office answered a permission)
  and `turnEnded` (the `session/prompt` result).
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
  `src-tauri/tests/cursor_e2e.rs` drives `fake-cursor`, an ACP agent written from
  the official schema plus the Cursor behaviour integrators report (`cursor_login`,
  hyphenated permission ids, `cursor/*` requests, both model APIs). Marker files
  in the project folder switch it to "logged out", "login fails" or "older model
  API"; words in the prompt pick a scripted turn (`permission`, `edit`, `plan`,
  `wait`, `fail`).
  *Restart* is covered in all three: `fake-claude` and `fake-codex` remember the
  sessions/threads they created (in their temporary config folder) and answer
  `--resume` / `thread/resume` like the real CLIs, including the real error for
  an unknown id; Cursor must refuse it.

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
* **Cursor**: not possible in the build environment (`cursor.com` is blocked).
  `*-sdk.json` fixtures copy the messages of the official ACP example agent
  (`@agentclientprotocol/sdk`, `dist/examples/agent.js`). To check ACP traffic
  against the official schema, record both directions of a session (for example
  with a wrapper script that `tee`s the agent's stdin and stdout) and parse each
  message with the SDK's `dist/schema/zod.gen.js` schemas. With a real Cursor,
  record the same way and add `*-real.json` fixtures.
* **Claude Code**: a temporary `CLAUDE_CONFIG_DIR`. Hook payloads and
  `stream-json` shapes can be captured this way; a real prompt needs an
  authenticated CLI and **costs tokens**, so it is never part of the tests.
  A temporary config folder alone does **not** keep a prompt free: credentials
  given through environment variables (for example when working inside another
  Claude Code session) are still used. Only record things that end before any
  API call (like the unknown `--resume` id), or start the CLI with a cleaned
  environment.

When a new CLI version changes a shape, re-record, update the fixture and the
mapping together, and note the finding in PROVIDER_CAPABILITIES.md.
* **UI tests** (Vitest): office layout validation, scene placement (project
  pods, subagents next to their lead, settle delay, exit) incl. 20 sessions + 50
  subagents and a logic speed check, sprite frames, team grouping, the renderer
  against a stand-in canvas, the stress generator, path finding, store merging,
  timestamp shifting, capability gating, which agents show the "silent"
  warning (and its hourglass in the renderer).
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
