# Implementation Plan, Risks and Vertical Slice

## 1. Minimal Vertical Slice (MVS)

The first release is complete when a Windows 11 user can:

1. Install `AgentOfficeSetup.exe` (no Node/Rust/Python needed) and open **Agent Office**.
2. See the pixel office with desks, meeting room, QA area, terminal area, lounge and
   CEO office.
3. Add a project (name + folder) and see its repository, branch and dirty state.
4. Open **Diagnostics** and see, for Claude Code, Codex and Cursor: installed?,
   version, integration status, and backend/database/Git status.
5. Click **Install integration** for Claude Code (safe merge into
   `%USERPROFILE%\.claude\settings.json` with backup), then run `claude` in any
   terminal and watch a character appear and change state in real time.
6. Launch a **managed** session for each provider from **New Agent**
   (provider, project, model) and see at least one character per provider.
7. Click a character to see provider, project, model, status, current action, elapsed
   time, branch, tool calls, commands, usage (or "Unavailable") and recent events;
   **Stop**, **Send prompt**, **Approve / Reject** where the capability exists.
8. Receive a Windows notification when an agent needs permission, finishes, or fails.
9. Close and reopen the app: projects, past sessions and events are still there.

Out of scope for the MVS: office editing, economy/upgrades, auto-update, code signing,
macOS/Linux packages, Cursor external (hook) sessions beyond "experimental".

## 2. Phases

Each phase ends with green tests and small commits. If a phase discovers that an
assumed API does not exist, we stop, update `PROVIDER_CAPABILITIES.md` and this plan,
then continue.

| Phase | Scope | Exit criteria |
| --- | --- | --- |
| **0 – Research & architecture** | CLI research, capability matrix, architecture, plan, risks | Docs merged (this commit set) |
| **1 – Desktop shell + core** | Cargo workspace, Tauri 2 app, React/Vite UI shell (Office, Command Center, Projects, Diagnostics views), `ao-core` event model + capabilities + adapter trait + registry + event bus + session reducer, `ao-store` SQLite + migrations + projects/sessions/events/preferences, `ao-detect`, provider descriptors with detection only, clearly-labelled Demo provider, UI batching channel, minimal office canvas, Windows CI build | `npm run test` green; `npm run build` produces an NSIS installer in CI on `windows-latest`; demo agents move in the office |
| **2 – Unified events** | Ingest pipeline (dedupe, redaction, truncation), per-session event log, ts-rs bindings, event-mapping test harness with fixtures | Mapping tests for every event type; 10 000-event load test without UI stalls |
| **3 – Claude adapter** | `ao-ipc` + hook relay (the app executable in `hook` mode); install/uninstall/repair/status of global hooks (merge, dedupe, backup, atomic write); external sessions via hooks; `claude agents --json` listing; managed sessions via `claude -p` stream-json + per-session `--settings`; approvals via `PermissionRequest` | Settings merge tests (existing hooks, plugins, invalid JSON, re-install idempotency); relay tests; managed session driven by a fake `claude` binary |
| **4 – Codex adapter** | app-server JSON-RPC client, thread/turn lifecycle, approvals, token usage, subagents; external hooks in `~/.codex/hooks.json` + trust status via `hooks/list`; `exec --json` fallback | Protocol tests against a fake app-server replaying recorded JSON-RPC; version pin checks |
| **5 – Cursor adapter** | ACP client (initialize/authenticate/session/new/prompt/cancel/request_permission, unknown-method handling), runtime capabilities; hooks as experimental; on-machine verification on Windows | ACP tests with a fake agent; manual verification checklist executed on Windows 11 |
| **6 – Office visualization** | Pixel sprites + animations per activity, zones, subagent spawn/leave, badges, selection, performance budget | 20 sessions + 50 subagents at 60 fps on a mid-range laptop |
| **7 – Process management** | `ao-process`: Job Objects, graceful stop, force kill, exit/stall/restart detection, `.cmd` shim handling, PID-reuse guard | Process tests on Windows CI with a fake agent (tree kill, no foreign kill) |
| **8 – Git integration** | `ao-git` (repo, branch, worktree, dirty/staged, ahead/behind, recent commits), refresh on tool/cwd events, conservative attribution | Tests on temporary repositories |
| **9 – Diagnostics & notifications** | Full diagnostics screen, observed-hook evidence, log viewer, Windows toast notifications with click-to-open | Diagnostics report exportable; notification click opens the session |
| **10 – Packaging** | NSIS `AgentOfficeSetup.exe` (Start Menu, desktop shortcut, uninstaller that first removes Agent Office hooks from provider settings), WebView2 bootstrapper, version metadata, optional signing hook, updater plumbing | Clean-VM install/uninstall smoke test |

## 3. Technical risks

| # | Risk | Impact | Mitigation |
| --- | --- | --- | --- |
| R1 | **Codex app-server is marked experimental** and its protocol changes often (0.156.x). | Managed Codex sessions break after a Codex update. | Pin tested version range, contract tests from `generate-json-schema`, tolerant deserialization (unknown fields/variants ignored), fallback to `codex exec --json`, clear Diagnostics message. |
| R2 | **Cursor CLI could not be executed during research**; native Windows support is recent; hook coverage in the CLI is reported incomplete. | Cursor features may differ on real machines. | Runtime capability negotiation via ACP `initialize`; hooks marked experimental; Phase 5 includes a Windows verification checklist; Diagnostics records observed events. |
| R3 | **Codex hook trust** requires a manual `/hooks` confirmation by the user. | External Codex sessions stay invisible until trusted. | Status via `hooks/list` `trustStatus`; UI explains the one-time step; never bypass trust for external sessions. |
| R4 | Editing `~/.claude/settings.json` could damage user config. | Lost settings/plugins. | Parse-or-refuse, timestamped backup, atomic write (temp + rename), only touch entries carrying our marker, idempotent install, uninstall/repair, tests with real-world settings fixtures. |
| R5 | A synchronous permission hook would hide the terminal prompt. | User confusion, stuck agents. | Observe-only by default; opt-in "answer from app" with bounded timeout and fallback to the terminal prompt. |
| R6 | Windows process trees (`.cmd` shims, `node` children, shells spawned by agents). | Orphans or wrong process killed. | Job Objects per managed session, kill only our jobs, PID + creation-time guard. |
| R7 | Hook latency slows the agent. | Worse agent UX. | Relay is the native app exe in `hook` mode (≈25 ms per call in a debug build when the app is closed), async hooks for non-decision events, 300 ms connect timeout, exit 0 when the app is closed. |
| R13 | App uninstalled while hooks remain in `~/.claude/settings.json`. | Claude reports a failing hook command on every event. | Phase 10: the NSIS uninstaller runs the hook removal first; until then Diagnostics → *Uninstall* must be used before removing the app (documented in the README). |
| R8 | Event floods (streaming deltas, command output). | UI jank, DB growth. | Batching (100 ms), UI never re-renders per token, output truncation, retention, virtualized lists. |
| R9 | Windows toast click-activation is limited in the Tauri notification plugin on desktop. | Click-to-open may not work in MVS. | Use WinRT toast activation (`tauri-winrt-notification`) in Phase 9; fall back to focusing the app. |
| R10 | Unsigned installer triggers SmartScreen. | Install friction. | Document; add signing step once a certificate exists. |
| R11 | Provider docs change faster than code. | Silent breakage. | Capabilities documented with versions; Diagnostics shows detected versions vs tested ranges. |
| R12 | Attributing Git changes to agents incorrectly. | Misleading UI. | Only attribute with tool-level evidence (file path in a completed edit tool call); otherwise show as "unattributed changes". |

## 4. Phase 1 checklist

- [x] Cargo workspace + `ao-core`, `ao-store`, `ao-detect`, provider crates (detection only), demo provider
- [x] Tauri 2 app wiring: host runtime, commands, event channel with 100 ms batching, rolling logs, headless `--smoke-test`
- [x] React shell: Office canvas, Command Center, Projects, Diagnostics, Agent panel, New Agent dialog
- [x] Unit tests (Rust + Vitest), GitHub Actions for Linux checks and Windows build/test/installer

### Phase 1 findings

* `CapabilityProfile` gained an `implemented` block so the UI can distinguish
  "the provider supports it" from "this build supports it" (no fake buttons).
* The browser preview replays batches recorded from Rust, avoiding a duplicated
  TypeScript reducer.
* The seat allocator needed overflow spots and a final any-free-seat fallback to
  hold 20 sessions + 50 subagents (caught by a unit test).
* Real providers are detection-only in this phase; launching/observing them is
  Phases 3–5 as planned. The demo provider exercises the full UI path meanwhile.

## 5. Phase 2 checklist

- [x] Ingest pipeline: validation → clock sanity → de-duplication → redaction/truncation → project resolution; rejected events counted in Diagnostics
- [x] Reducer hardening: out-of-order tool events, answered/expired permissions, waiting-for-input state, no agents created by trailing events
- [x] `ao-testkit` fixture harness (`fixtures/**`, subset matching) and `docs/EVENT_MODEL.md`
- [x] 10 000-event load test (pipeline + world + batcher) and a round trip of every event type

## 6. Phase 3 checklist

- [x] `ao-process`: process trees in Job Objects / process groups, graceful stop, tree kill, never foreign processes
- [x] `ao-ipc`: token-authenticated named pipe (Unix socket in development), length-prefixed frames
- [x] Hook relay: `agent-office hook <provider>`, fail-open, bounded waits
- [x] Claude hooks → unified events for every documented event we subscribe to (fixtures)
- [x] Global integration: install / repair / uninstall / status with merge, de-duplication, backups, atomic write, invalid-JSON protection
- [x] External sessions via hooks + discovery with `claude agents --json`
- [x] Managed sessions: `claude -p` stream-json, per-session `--settings`, prompts, stop, usage (cost as estimate)
- [x] Permissions: always answerable for managed sessions; opt-in for external ones with timeout fallback
- [x] Diagnostics: hook bridge, events received, integration buttons, settings; New Agent: Claude models and permission modes
- [x] End-to-end tests with `fake-claude` (managed session; global integration over existing user settings)

### Phase 3 findings

* Claude merges `--settings` hooks with user hooks, and both the `Stop` hook and the
  stream `result` report the end of a turn in `-p` mode; both would have produced
  duplicates and are handled explicitly (see PROVIDER_CAPABILITIES §3.5).
* `PermissionRequest` has no `tool_use_id`: permissions are tied to the session/agent,
  not to a tool call.
* The relay is the app executable itself (`mainBinaryName` pinned), which removes a
  sidecar from packaging; a standalone `agent-office-hook` binary exists for tests.
* A final check against the real CLI (managed session + global hooks for an external
  session) matched the fake-claude tests. That check ran two tiny real prompts; the
  automated tests never use a real account.
* Next: Codex (Phase 4). Hook removal in the uninstaller moves to Phase 10 (risk R13).
