# Architecture

Agent Office is a local-first Windows desktop app that turns running coding agents
(Claude Code, Codex CLI, Cursor CLI, and future providers) into characters in a
pixel-art office. This document records the architecture and the reasoning behind it.
Provider-specific facts live in [PROVIDER_CAPABILITIES.md](PROVIDER_CAPABILITIES.md).

## 1. Desktop stack decision: Tauri 2 vs Electron

| Criterion | Tauri 2 (Rust + WebView2) | Electron (Node + bundled Chromium) |
| --- | --- | --- |
| Memory at idle | ~40–90 MB (shared WebView2 runtime) | ~150–300 MB (own Chromium + Node) |
| Startup | Fast; small native binary | Slower; loads Chromium + Node runtime |
| Installer size | ~5–15 MB | ~80–150 MB |
| Native process access | Rust: Win32 Job Objects, named pipes, `CreateProcessW` flags via the `windows` crate | Node `child_process`; Job Objects/named-pipe ACLs need native addons |
| Security model | Renderer has no OS access; every command is allow-listed via capabilities | Needs careful `contextIsolation`/preload hardening |
| Packaging | Built-in NSIS/MSI bundler → `AgentOfficeSetup.exe`, Start Menu + desktop shortcut + uninstaller | electron-builder, similar |
| Auto-update | `tauri-plugin-updater` (signed updates) | `electron-updater` |
| IPC | Typed commands, events, and streaming **Channels** | `ipcMain`/`ipcRenderer` |
| Windows 11 | WebView2 is preinstalled on Windows 11 | Works everywhere |
| Weak points | Rust learning curve; WebView differences across OSes (irrelevant on Windows-first) | Resource usage; larger attack surface |

**Decision: Tauri 2.** The app's core job — supervising long-running child
processes, owning named pipes, reacting to OS events — is systems work where Rust is
the better tool, and the resource profile matters for an app that stays open all day
next to heavy agents. Electron's advantages (single language, huge ecosystem) do not
outweigh this.

Rest of the stack, deliberately small:

* **UI:** React 19 + TypeScript + Vite. No Next.js: there is no server rendering or
  routing need inside a desktop webview.
* **Office rendering:** our own Canvas 2D renderer with a fixed-rate loop outside React.
  20 agents + 50 subagents is far below what Canvas 2D handles; a WebGL engine
  (PixiJS) can replace the renderer later behind the same `OfficeRenderer` interface.
* **UI state:** a small normalized store (Zustand); the canvas reads snapshots and
  never triggers React renders.
* **Persistence:** SQLite via `rusqlite` with the `bundled` feature (no system SQLite).
* **Package manager:** npm (ships with Node; nothing extra to install).
* **Scripts:** `package.json` scripts and cross-platform Node `.mjs` tools. No Make, no
  Bash, no WSL.

## 2. System overview

```
 ┌──────────────── Provider CLIs (child processes or user terminals) ───────────────┐
 │  claude -p (stream-json)   codex app-server (JSON-RPC)   agent acp (ACP JSON-RPC) │
 │  claude / codex / agent in the user's own terminal ──► hooks ──► agent-office hook│
 └──────────┬───────────────────────┬──────────────────────────┬───────────────┬────┘
            │ stdio                 │ stdio                    │ stdio         │ named pipe
 ┌──────────▼───────────────────────▼──────────────────────────▼───────────────▼────┐
 │ Rust core (inside the Tauri process)                                              │
 │  ┌─────────────────── Provider adapters (isolated tasks, supervised) ───────────┐ │
 │  │ ClaudeAdapter │ CodexAdapter │ CursorAdapter │ DemoAdapter │ FutureAdapter   │ │
 │  └──────┬────────────────────────────────────────────────────────────────────── ┘ │
 │         │ AgentEvent (normalized)                                                 │
 │  ┌──────▼──────────┐   ┌─────────────────┐   ┌──────────────┐                     │
 │  │ Unified EventBus │──►│ Session Manager │──►│ State Store  │ (in-memory model)  │
 │  └──────┬──────────┘   └────────┬────────┘   └──────────────┘                     │
 │         │                       │ uses                                            │
 │         │        ┌──────────────┼───────────────┬──────────────┬──────────────┐   │
 │         │        │ ProcessManager│ GitService   │ UsageService  │ Notifications│   │
 │         │        └──────────────┴───────────────┴──────────────┴──────────────┘   │
 │         ├──► SQLite writer (batched transactions, retention)                      │
 │         └──► UI batcher (≈10 Hz) ──► Tauri Channel                                │
 └───────────────────────────────────────────────────────────────┬──────────────────┘
                                                                 │ Tauri IPC (no sockets)
 ┌───────────────────────────────────────────────────────────────▼──────────────────┐
 │ React UI: Office (canvas) · Command Center · Projects · Diagnostics · Agent panel │
 └──────────────────────────────────────────────────────────────────────────────────┘
```

The UI consumes **only normalized events and state snapshots**. No UI code knows
what a Claude hook or a Codex JSON-RPC notification looks like.

## 3. Repository layout

```
AI-Agent-Office/
├─ package.json            # npm scripts: dev, build, test, lint, typecheck
├─ Cargo.toml              # Rust workspace
├─ index.html, vite.config.ts, tsconfig.json
├─ src/                    # React UI (TypeScript)
│  ├─ ipc/                 # typed wrappers over Tauri invoke/Channel (+ browser preview backend)
│  ├─ state/               # normalized store, event reducer
│  ├─ office/              # canvas renderer, layout, sprites, scene logic
│  ├─ views/               # Office, Command Center, Projects, Diagnostics
│  ├─ components/          # Agent panel, shared widgets
│  └─ bindings/            # TS types generated from Rust (ts-rs)
├─ src-tauri/              # Tauri app crate: wiring, commands, windows, tray, notifications
├─ crates/
│  ├─ ao-core/             # event model, capabilities, ProviderAdapter trait, registry,
│  │                       # event bus, session reducer, redaction  (no OS / no Tauri deps)
│  ├─ ao-store/            # SQLite schema, migrations, batched writer, retention
│  ├─ ao-detect/           # executable discovery + version probing (PATHEXT aware)
│  ├─ ao-process/          # owns managed agents' process trees (Job Objects / process groups)
│  ├─ ao-ipc/              # local IPC (named pipe / Unix socket) + token, shared by app and relay
│  ├─ ao-hook-relay/       # hook relay (`agent-office hook …`) + standalone binary for tests
│  ├─ ao-config/           # safe edits of provider config files (backup, atomic write)
│  ├─ ao-jsonrpc/          # JSON-RPC 2.0 client over a managed process's stdio (Codex app-server, ACP)
│  ├─ ao-testkit/          # fixture harness, fake provider CLIs (fake-claude, fake-codex, fake-cursor), cargo_bin helper
│  ├─ ao-git/              # GitService: read-only git.exe (porcelain v2) + parsers
│  └─ providers/
│     ├─ ao-provider-claude/
│     ├─ ao-provider-codex/
│     ├─ ao-provider-cursor/
│     └─ ao-provider-demo/ # clearly labelled simulated provider for UI dev & tests
├─ fixtures/               # recorded/synthetic provider payloads used by tests
├─ scripts/                # cross-platform Node tools (.mjs)
└─ docs/
```

Adding a provider = a new crate under `crates/providers/`, one registration line in
`src-tauri/src/providers.rs`, fixtures and tests. The UI needs no change: provider
name, accent color and badge come from the adapter's `ProviderDescriptor`.
See [ADDING_A_PROVIDER.md](ADDING_A_PROVIDER.md).

## 4. Core abstractions (`ao-core`)

### 4.1 ProviderAdapter

```rust
#[async_trait]
pub trait ProviderAdapter: Send + Sync + 'static {
    fn descriptor(&self) -> ProviderDescriptor;          // id, display name, accent, badge, simulated?
    fn capabilities(&self) -> CapabilityProfile;         // managed + external + what this build implements
    async fn detect_installation(&self) -> InstallationInfo;   // never fails: errors are reported inside
    async fn get_version(&self) -> Option<String>;
    async fn launch_session(&self, req: LaunchRequest, ctx: AdapterContext) -> Result<SessionHandle, ProviderError>;
    async fn attach_to_session(&self, id: &SessionId, ctx: AdapterContext) -> Result<(), ProviderError>;
    async fn stop_session(&self, id: &SessionId, mode: StopMode) -> Result<(), ProviderError>;
    async fn send_prompt(&self, id: &SessionId, prompt: &str) -> Result<(), ProviderError>;
    async fn resolve_permission(&self, id: &SessionId, request: &PermissionRequestId, decision: PermissionDecision) -> Result<(), ProviderError>;
    async fn list_sessions(&self) -> Result<Vec<ExternalSessionInfo>, ProviderError>;
    async fn integration_status(&self, ctx: &AdapterContext) -> IntegrationStatus;
    async fn install_integration(&self, ctx: &AdapterContext) -> Result<IntegrationStatus, ProviderError>;
    async fn uninstall_integration(&self, ctx: &AdapterContext) -> Result<IntegrationStatus, ProviderError>;
    async fn repair_integration(&self, ctx: &AdapterContext) -> Result<IntegrationStatus, ProviderError>;
    fn configure(&self, settings: &ProviderSettings);            // user preferences (default: ignore)
    async fn start(&self, ctx: AdapterContext);                  // background work, e.g. session discovery
    async fn handle_hook(&self, call: HookCall, ctx: AdapterContext) -> HookReply; // default: empty reply
}
```

`AdapterContext` carries the `EventSink`, the `RelayCommand` agent CLIs must run to
reach Agent Office (`None` when hooks are unavailable) and the data folder (for
per-session scratch files such as Claude's `--settings` file).

Every method except `descriptor`/`capabilities` has a default implementation that
returns `ProviderError::Unsupported { capability }`. `approvePermission` /
`rejectPermission` are one method with a `PermissionDecision` argument.
`subscribeEvents` is not a method: adapters receive an `EventSink` in
`AdapterContext` and push normalized events into the bus.

### 4.2 Capabilities

```rust
pub enum Support { Supported, Partial, Experimental, Runtime, Unsupported }
pub struct Capabilities {
    pub launch, attach, list_sessions, stop, send_prompt, structured_events,
        tool_events, file_events, command_events, permissions, subagents,
        usage, cost, model, context_compaction, resume: Support,
}
pub struct CapabilityProfile {
    pub managed: Capabilities,
    pub external: Capabilities,
    pub implemented: ImplementedFeatures, // detection / managed / external / integration setup
    pub notes: Vec<String>,
}
```

`capabilities` describes what the **provider** offers; `implemented` says what
**this build** of Agent Office can already use. The host refuses an action (and
the UI disables its button with the reason) unless both are true. This keeps
unfinished phases honest: e.g. Claude managed sessions are `Supported` by Claude
Code but not implemented until Phase 3.

The UI enables a button only when the relevant capability of the *session's mode*
is available. Runtime-negotiated values (ACP `initialize`) are stored per session.

### 4.3 Unified event model

Every event is an `AgentEvent` envelope:

| Field | Meaning |
| --- | --- |
| `eventId` | UUID v4, generated at ingestion |
| `timestamp` | ms since epoch (time the provider/relay observed it) |
| `provider` | `"claude"`, `"codex"`, `"cursor"`, `"demo"`, … |
| `sessionId` | Provider session id (Claude `session_id`, Codex `threadId`, ACP `sessionId`) |
| `agentId` | Main agent = session id; subagent = provider's subagent id |
| `parentAgentId` | Set for subagents |
| `projectId` / `repositoryId` | Resolved by the Session Manager from `cwd` |
| `source` | `hook` \| `protocol` \| `process` \| `git` \| `internal` \| `simulation` |
| `kind` + `payload` | One of the event types below, serialized as `{ "type": "tool.started", "payload": {…} }` |

Event types: `session.started`, `session.updated`, `session.ended`,
`prompt.submitted`, `agent.thinking`, `agent.idle`, `agent.waiting`, `agent.error`,
`agent.message`, `tool.started`, `tool.completed`, `tool.failed`, `file.read`,
`file.created`, `file.modified`, `file.deleted`, `command.started`,
`command.output`, `command.completed`, `command.failed`, `permission.requested`,
`permission.approved`, `permission.denied`, `subagent.started`, `subagent.updated`,
`subagent.ended`, `git.branch_changed`, `git.commit_created`, `git.status_changed`,
`context.compacted`, `usage.updated`, `provider.error` (adapter health, internal).

TypeScript types are **generated from the Rust definitions** (`ts-rs`) into
`src/bindings/` so both sides cannot drift.

### 4.4 Session model and activity

The Session Manager folds events into `SessionState` / `AgentState` with a pure
reducer (`ao_core::session::apply`). Activity shown in the office:

| Activity | Driven by |
| --- | --- |
| `IDLE` | `agent.idle`, `Stop`, session start |
| `THINKING` | `prompt.submitted`, `agent.thinking`, tool completed with nothing running |
| `READING` | tool category `read` / `search` / `fetch` |
| `CODING` | tool category `edit` |
| `RUNNING_COMMAND` | tool category `execute` |
| `TESTING` | `execute` whose command matches a test-runner pattern (display heuristic) |
| `WAITING_PERMISSION` | `permission.requested`, `agent.waiting` |
| `ERROR` | `agent.error`, `tool.failed` streak, process crash |
| `DONE` | `session.ended` (brief animation, then the character leaves) |

## 5. Integration channels

### 5.0 Browser preview

`npm run dev:web` runs the UI without the desktop shell. Instead of a second
TypeScript implementation of the reducer, it replays UI batches **recorded by the
Rust demo provider on a virtual clock** (`src/ipc/preview-recording.json`). The
preview is clearly labelled and cannot control agents.

### 5.1 UI ↔ core: Tauri IPC only

Commands (`invoke`) for request/response, one **Channel** for the event stream.
The core batches state patches and events and flushes at most every 100 ms, so a
burst of 1 000 events is one UI update, not 1 000.

### 5.2 Hooks ↔ core: `agent-office hook` over a named pipe

Hooks are configured as **exec-form commands** (`command` + `args`, no shell, no Bash,
no PowerShell scripts) pointing at the **Agent Office executable itself** in relay
mode:

```
"C:\...\Agent Office\agent-office.exe" hook claude --origin global [--wait 150] [--data-dir <dir>]
```

Using the main executable avoids bundling and versioning a second sidecar; its file
name is pinned (`mainBinaryName: agent-office`) so installed hooks stay
recognisable. The relay (`ao-hook-relay`):

1. reads the hook JSON from stdin (capped at 4 MiB; larger payloads are truncated
   and flagged),
2. connects to `\\.\pipe\agent-office-v1-<hash of the data folder>` (created with
   `first_pipe_instance` and `reject_remote_clients`; a 0600 Unix socket on
   development machines) and authenticates with the per-user token in
   `<data folder>\ipc.token`,
3. sends one length-prefixed JSON frame (`HookRequest`: provider, origin
   `global`/`managed`, timestamp, payload) and waits for the answer at most
   `--wait` seconds (5 s by default; permission requests use the configured answer
   timeout),
4. prints the answer's stdout (e.g. a Claude permission decision) and exits with its
   code,
5. **if the app is not running or anything fails, exits 0 with no output** (300 ms
   connect timeout), so the agent always continues normally.

In the app, the `HookBridge` counts every hook event per provider (Diagnostics →
*Hook bridge*: the empirical record of what each CLI fires on this machine) and
passes the call to `ProviderAdapter::handle_hook`. No TCP port is opened. (Claude
also supports `http` hooks; we do not use them to avoid a listening port.)

### 5.3 Managed sessions

| Provider | Process | Protocol |
| --- | --- | --- |
| Claude | `claude -p --input-format stream-json --output-format stream-json --verbose --session-id <uuid> --settings <hooks-json>` | stream-json on stdio + hooks via relay (permissions answered via `PermissionRequest` hook) |
| Codex | `codex app-server` (stdio), one process and one root thread per session | JSON-RPC 2.0 (protocol v2); approvals are server→client requests answered from the UI; subagents are child threads |
| Cursor | `agent acp` | ACP JSON-RPC 2.0 |

Codex managed sessions need no hooks: the app-server protocol reports
everything, including approvals. Codex still runs the user's own hooks inside
those sessions, so the Codex adapter ignores hook calls for threads it manages.

Claude managed sessions always get their own hooks through a per-session
`--settings` file (origin `managed`, permission requests answered from the app).
Claude **merges** those with the user's hooks (verified with 2.1.281), so when the
global integration is installed both fire; the adapter drops `global` calls for
sessions it launched (the id is reserved before the process starts). Within a
managed Claude session the two channels split the work:

| From hooks | From stream-json |
| --- | --- |
| prompts, tool calls, files, commands, subagents, permission requests, compaction, session start/end | model (`system/init`), assistant text, API retries and errors, token usage and cost estimate (`result`), end of turn |

## 6. Process Manager (Windows-first)

Implemented in `ao-process` (Phases 3 and 7):

* Spawns through `tokio::process` with `CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP`.
  `.cmd` shims (npm installs) are started by Rust's standard library, which runs
  them through `cmd.exe` with its own argument escaping and refuses arguments it
  cannot escape safely; Agent Office puts nothing user-typed on command lines
  (prompts go through stdin, hook settings through a file). A Windows CI test
  starts a real `.cmd` shim, checks its arguments and exit code, and kills its tree.
* Each managed process gets its **own Job Object**
  (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`). The process is created **suspended**
  (`CREATE_SUSPENDED`), put in the job, then resumed: it runs its first
  instruction inside the job, so nothing it starts can escape. If it cannot be
  resumed it is killed rather than left behind.
* Captures stdout/stderr line by line (and remembers when the last line came),
  owns stdin, and keeps the process handle for its whole life, so it never acts
  on a PID that could have been reused.
* **Stop** = close stdin → wait 10 s → terminate the job. **Force stop**
  terminates the job at once. Dropping a `ManagedProcess` also terminates its tree.
* **Exit details** (`ExitInfo`): exit code, Unix signal, whether Agent Office
  stopped it, and how many processes the agent left running when it exited
  (they are stopped: they belong to the finished session). `describe()` turns
  this into the session's end reason ("finished", "exited with code 2",
  "crashed (access violation), code 0xC0000005", "stopped by Agent Office;
  2 leftover processes stopped").
* A **registry** lists every process Agent Office started and still tracks
  (label, PID, start, status, live processes in its tree, last output):
  Diagnostics → *Processes started by Agent Office*.
* **Never kills a process it did not create.** Only our own jobs / process
  groups are ever terminated. External sessions have no kill action (except
  Claude background sessions via the official `claude stop <id>`).
* Non-Windows builds use process groups (`killpg`) behind the same API.

Built on top of it:

* **Silent agents.** When an agent is busy (thinking, reading, coding, running a
  command or tests) and nothing has been heard from its session for N minutes
  (preference, default 5, 0 = off), the session gets `silentSince`. The office
  shows an hourglass and the panel explains it. It is only a warning: a long
  build or test run is normal, so nothing is stopped automatically.
* **Restart counter.** A session that starts again after it ended (Restart, or
  `--resume` in a terminal) increments `restarts`.
* **Restart** (managed sessions): stops the session gracefully if it is running,
  then continues the same conversation in a new process through the provider's
  official resume: Claude `claude -p --resume <id>`, Codex `thread/resume`. The
  folder, model and permission mode are kept. Cursor: not implemented (its ACP
  `session/load` is optional and could not be tested without a real Cursor).
* **Open terminal**: opens the user's own terminal in the session's folder
  (Windows Terminal `wt.exe -d <folder>`, else PowerShell or `cmd` in a new
  console with that working directory). It is started detached and never
  managed; no command is typed into it, and the folder is passed as one argument.

## 6a. Git Service (`ao-git` + `src-tauri/src/git.rs`)

* **Reads only**, through the user's own `git` (Git for Windows' `git.exe`;
  found like Diagnostics finds it): `rev-parse` (which working tree and
  repository a folder belongs to), `status --porcelain=v2 --branch -z`
  (branch, upstream, ahead/behind, staged / not staged / new / conflicted
  files), `log` (recent commits, commits between two heads) and
  `worktree list --porcelain`. Nothing is ever written to a repository.
* **Out of the agents' way:** `--no-optional-locks` (status never rewrites the
  index, so it never holds `index.lock` while an agent commits), no pager,
  colors or credential prompts, no console window, 20 s timeout, and none of
  the caller's `GIT_*` variables.
* **Safe with untrusted repositories as far as Git allows:** the repository's
  `core.fsmonitor` program is not started (a real `git status` would start
  it); only checked commit ids are passed as revisions; Git's own
  `safe.directory` check stays on (a repository owned by another Windows user
  is reported as refused). Like any Git tool, other repository settings (for
  example content filters) still apply, as they do when the agent itself runs
  `git`.
* **When:** a session is linked to the working tree of its folder when it
  starts. The tree is re-read 1.5 s after the agent edits files, runs a
  command or finishes a turn (debounced), every 30 s while an agent works
  there, and when the Projects view asks (at most every 5 s). Project folders
  are read when the Projects view is open.
* **Events:** changes become `git.branch_changed`, `git.status_changed` and
  the linked worktree for every active session in the tree, with the
  repository id (the main working tree's folder) on each event.
* **Attribution (conservative):** a commit is credited to a session only when
  that session ran a commit-creating `git` command in that tree and the
  commit's time falls within that command's run; with no such session, or
  several, the commit is credited to nobody. A changed file is linked to a
  session only when that session's tool reported writing it
  (`file.created` / `file.modified` / `file.deleted`). Nothing is inferred
  from timing alone or from who is "nearby".
* **UI:** `list_repositories` returns each tree (status, files with their
  writers, recent commits with their credited session, worktrees, sessions,
  projects). *Open project* opens the folder in the file manager; *Show* a
  changed file selects it in Explorer (`explorer /select,`). Files are never
  opened with their default program: on Windows that would run `.js`, `.bat`
  or `.exe` files. Only folders of sessions, projects and followed
  repositories can be opened, and paths cannot escape them.

## 7. Persistence

SQLite at `%LOCALAPPDATA%\AgentOffice\agent-office.db` (WAL mode). Tables:
`projects`, `sessions`, `agents`, `events`, `preferences`, `layouts`,
`integrations`, `usage_snapshots`, `schema_migrations`.

* A single writer task batches inserts into one transaction per flush (≤100 ms).
* Retention (configurable): events older than *N* days (default 14) are purged;
  large payload fields (command output, tool responses) are truncated at ingest
  (default 8 KB) with a `truncated` flag.
* Secrets: provider credentials are never read or stored. A redaction pass masks
  strings that look like API keys/tokens before events are persisted or logged.

## 8. Logging and diagnostics

* `tracing` with two rolling files in `%LOCALAPPDATA%\AgentOffice\logs\`:
  `app.log` (application) and `providers.log` (adapter/protocol traffic, redacted).
  Levels DEBUG/INFO/WARN/ERROR, configurable.
* Diagnostics screen: installation + version per provider, integration/hook status,
  backend status, database status (path, size, schema version, integrity check),
  Git detection, all relevant paths, adapter health, and hook events actually observed
  per provider. "Run diagnostics" re-runs everything.

## 9. Fault isolation

Each adapter runs in its own supervised Tokio tasks. A panic or error in one adapter
is caught at the task boundary, converted into a `provider.error` internal event,
shown in Diagnostics, and the adapter is restarted with exponential backoff. Other
providers keep running. Malformed provider payloads are logged and dropped, never
propagated as panics.

## 10. Office visualization

Code: `src/office/` — `layout.ts` (data), `scene.ts` (placement and movement, no
drawing), `sprites.ts` (pixel art), `renderer.ts` (Canvas 2D), `teams.ts`
(project grouping), `OfficeView.tsx` (frame loop, hover card, keyboard, legend).

* **Data-driven layout** (`OfficeLayout`): `rooms` (open space, meeting room, QA
  lab, terminal room, lounge, CEO office), `furniture`, `decorations`, `seats` and
  `pods` (rows of desks a project team shares). `validateLayout` checks ids, walls,
  overlaps and that every seat can be reached from the entrance, so an edited or
  saved layout can be refused before use. The same model will later support moving
  furniture, `OfficeUpgrade`, `Perk` and unlockables (types only, no economy); the
  `layouts` table in SQLite is reserved for saved layouts.
* **Placement** (`OfficeScene`, pure and unit-tested):
  * activity → zone: coding / reading / thinking / waiting → own desk, running a
    command → terminal room, testing → QA lab, idle → lounge, finished → exit;
  * each project team gets a pod (first free row; teams share when all rows are
    taken); a lead keeps its desk while it visits other rooms; subagents take the
    free desk nearest their lead and overflow into the meeting room;
  * a character changes room only after the new activity lasted
    `ZONE_SETTLE_MS` (1.5 s), so short tool calls do not make it run back and forth;
  * finished agents celebrate for 1.8 s, walk to the door and fade out.
* **Teams**: the session's project (resolved by the core, else the registered
  project whose folder contains the session's folder), else the folder name.
* **Characters**: original 12 × 16 pixel sprites composed from a hair style, a
  torso pose and legs (`characterRows`), cached per look and pose. Poses per
  activity: typing (two frames), reading a sheet, hand on chin, raised hand
  (waiting), hands on the head with a drop of sweat (error), coffee (idle),
  celebrating, walking. Looks are derived from the agent key; the shirt is the
  provider's accent color (lighter for subagents), the badge its short name. No
  third-party logos.
* **Signals**: bubbles (always for leads and anyone needing attention, for other
  subagents only when their team is focused), a thought cloud for thinking,
  screens that show code / a document / a terminal / an alert / an error for the
  person using them, pulsing red rings for attention, confetti when done,
  dotted lead → subagent links (always in a small office, only for the focused
  team when busy), the CEO desk's inbox counting pending approvals.
* **Interaction**: hover (or arrow keys on the focused canvas) shows a card with
  name, provider, project, status, elapsed time and current action; click or
  Enter opens the agent panel; Escape clears; **Legend** explains the symbols.
* **Rendering**: its own `requestAnimationFrame` loop reads the store with
  `getState()` (React never re-renders the canvas), draws at 60 fps while someone
  walks and 30 fps when the office is calm, caches the static floor, character
  frames, looks and text widths.
* **Budget**: 20 sessions + 50 subagents. Measured with
  `scripts/office-bench.mjs` on the preview's stress test (headless Chromium, 4
  vCPU, software rendering): about 1 ms per frame on average, 1.5 ms at the 95th
  percentile, 59–60 fps drawn, both at 1600 × 1000 and at 1920 × 1080 × 2
  (HiDPI). The frame budget at 60 fps is 16.7 ms.

## 11. Security summary

Local-only: no listening TCP sockets, named pipe restricted to the local user,
hook relay authenticated with a per-user token, Tauri capabilities allow-list, no
telemetry, no upload of prompts/code/tool output. See [SECURITY.md](SECURITY.md).

## 12. Portability

Windows 11 x64 is the target. All OS-specific code sits behind small interfaces
(`ao-process` job control, `ao-ipc` local socket naming, `ao-detect` executable
lookup) with Unix implementations used for development and CI on Linux/macOS.
