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
 │  claude / codex / agent in the user's own terminal ──► hooks ──► agent-office-hook│
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
│  ├─ ao-process/          # process manager (Job Objects on Windows)            [Phase 7]
│  ├─ ao-ipc/              # named-pipe protocol shared by app and hook relay    [Phase 3]
│  ├─ ao-hook-relay/       # `agent-office-hook.exe` sidecar                     [Phase 3]
│  ├─ ao-git/              # GitService (git.exe porcelain v2)                   [Phase 8]
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
pub trait ProviderAdapter: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;          // id, display name, accent, badge
    fn capabilities(&self) -> CapabilityProfile;         // managed + external, honest values
    async fn detect_installation(&self) -> Result<InstallationInfo, ProviderError>;
    async fn get_version(&self) -> Result<Option<String>, ProviderError>;
    async fn launch_session(&self, req: LaunchRequest, ctx: AdapterContext) -> Result<SessionHandle, ProviderError>;
    async fn attach_to_session(&self, id: &SessionId, ctx: AdapterContext) -> Result<(), ProviderError>;
    async fn stop_session(&self, id: &SessionId, mode: StopMode) -> Result<(), ProviderError>;
    async fn send_prompt(&self, id: &SessionId, prompt: &str) -> Result<(), ProviderError>;
    async fn resolve_permission(&self, id: &SessionId, request: &PermissionRequestId, decision: PermissionDecision) -> Result<(), ProviderError>;
    async fn list_sessions(&self) -> Result<Vec<ExternalSessionInfo>, ProviderError>;
    async fn integration_status(&self) -> Result<IntegrationStatus, ProviderError>;
    async fn install_integration(&self) -> Result<IntegrationReport, ProviderError>;
    async fn uninstall_integration(&self) -> Result<IntegrationReport, ProviderError>;
    async fn repair_integration(&self) -> Result<IntegrationReport, ProviderError>;
    async fn handle_hook(&self, input: HookEnvelope) -> Result<HookOutcome, ProviderError>;
}
```

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
pub struct CapabilityProfile { pub managed: Capabilities, pub external: Capabilities, pub notes: Vec<String> }
```

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

### 5.1 UI ↔ core: Tauri IPC only

Commands (`invoke`) for request/response, one **Channel** for the event stream.
The core batches state patches and events and flushes at most every 100 ms, so a
burst of 1 000 events is one UI update, not 1 000.

### 5.2 Hooks ↔ core: `agent-office-hook.exe` over a named pipe

Hooks are configured as **exec-form commands** pointing at the bundled sidecar
`agent-office-hook.exe <provider>` (no shell, no Bash, no PowerShell scripts). The
relay:

1. reads the hook JSON from stdin (size-capped),
2. connects to `\\.\pipe\agent-office-<user-hash>` (`PIPE_REJECT_REMOTE_CLIENTS`),
   authenticates with the per-user token stored in `%LOCALAPPDATA%\AgentOffice\ipc.token`,
3. sends a length-prefixed JSON frame,
4. for decision-capable events in "answer from app" mode, waits for a reply (bounded),
   then prints the provider-specific JSON output,
5. **if the app is not running or anything fails, exits 0 with no output within
   ~200 ms**, so the agent always continues normally.

No TCP port is opened. (Claude also supports `http` hooks; we do not use them to
avoid a listening port.)

### 5.3 Managed sessions

| Provider | Process | Protocol |
| --- | --- | --- |
| Claude | `claude -p --input-format stream-json --output-format stream-json --verbose --session-id <uuid> --settings <hooks-json>` | stream-json on stdio + hooks via relay (permissions answered via `PermissionRequest` hook) |
| Codex | `codex app-server` (stdio) | JSON-RPC 2.0, protocol v2 |
| Cursor | `agent acp` | ACP JSON-RPC 2.0 |

If global Claude hooks are installed, managed sessions do not inject a second copy;
the ingest layer also de-duplicates by `(provider, session, hook event, tool_use_id)`.

## 6. Process Manager (Windows-first)

* Spawns with `CreateProcessW` semantics through `tokio::process` plus Windows flags
  (`CREATE_NO_WINDOW`, `CREATE_NEW_PROCESS_GROUP`); resolves `.cmd` shims via
  `cmd.exe /d /s /c` with proper argument quoting.
* Each managed process is assigned to its **own Job Object**
  (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`): the whole tree (agent + shells it spawned)
  is terminated with the job, and nothing outside the job can be affected.
* Captures stdout/stderr line-by-line, owns stdin, records PID **and process creation
  time** (PID reuse guard), exit code, and restart count.
* Stop = protocol-level cancel → close stdin → wait (grace period) → `TerminateJobObject`.
* Liveness: exit detection via process handle; **stalled** detection when a session
  says it is busy but nothing arrived for N minutes (shown as a warning, never auto-killed).
* **Never kills a process it did not create.** External sessions have no kill action
  (except Claude background sessions via the official `claude stop <id>`).
* Non-Windows builds use process groups (`setsid`/`killpg`) behind the same API.

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

* Data-driven layout (`OfficeLayout`): `rooms` (desks, meeting, QA, terminal area,
  lounge, CEO office), `furniture`, `decorations`, and `seats`. The same model will
  later support moving furniture, `OfficeUpgrade`, `Perk` and unlockables — only the
  data types exist now; no economy.
* Characters are procedurally drawn pixel sprites (original art, no third-party logos),
  tinted with the provider accent and labelled with a badge.
* Placement: activity → zone (coding/reading/thinking → desk, running command →
  terminal area, testing → QA, idle → lounge). Subagents spawn next to their parent,
  are linked by a thin line, and walk out when they end.
* The renderer runs its own `requestAnimationFrame` loop reading a snapshot of the
  store; React re-renders only side panels.

## 11. Security summary

Local-only: no listening TCP sockets, named pipe restricted to the local user,
hook relay authenticated with a per-user token, Tauri capabilities allow-list, no
telemetry, no upload of prompts/code/tool output. See [SECURITY.md](SECURITY.md).

## 12. Portability

Windows 11 x64 is the target. All OS-specific code sits behind small interfaces
(`ao-process` job control, `ao-ipc` local socket naming, `ao-detect` executable
lookup) with Unix implementations used for development and CI on Linux/macOS.
