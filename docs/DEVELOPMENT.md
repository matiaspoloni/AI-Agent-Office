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
| `npm run smoke` | Headless smoke test of the desktop binary (`--smoke-test`) |
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
crates/providers/*    one crate per provider (claude, codex, cursor, demo)
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
| Override data folder | `AGENT_OFFICE_DATA_DIR=<folder>` |
| Log level | `AGENT_OFFICE_LOG=debug` (or `info,provider=debug`) |

## Tests

* **Rust unit/integration tests** live next to the code (`cargo test -p <crate>`).
  They cover the event model, pipeline, reducer, redaction, SQLite store and
  writer, detection, capability declarations, the demo provider (permissions,
  stop), preview recording, and the host runtime (event → state → UI → disk,
  restart behaviour).
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
