# Provider Capabilities

Status of each supported coding-agent CLI, as researched in **Phase 0 (2026-09-24)**.
This document is the source of truth for the `Capabilities` each adapter reports.
**Rule: if a row says "No", the adapter must return `unsupported` for it. We never
simulate a capability.**

Legend:

| Mark | Meaning |
| --- | --- |
| **Yes** | Official, documented mechanism; verified against docs, CLI help or generated protocol schema. |
| **Partial** | Available with a documented limitation (see notes). |
| **Experimental** | Mechanism exists officially but is marked experimental upstream, or community reports say it is incomplete. Needs on-machine verification; shown with an "experimental" badge in the UI. |
| **Runtime** | Negotiated at runtime (e.g. an ACP `initialize` response); the adapter refines the value per session. |
| **No** | Not available through any supported interface. Not implemented. |

Two integration modes exist for every provider:

* **Managed session** – Agent Office launches the agent process itself and talks to it
  through a structured protocol on stdio.
* **External session** – the user launched `claude` / `codex` / `agent` in their own
  terminal. Agent Office can only observe what the provider pushes to it (hooks) or
  exposes through an official listing command. It never kills or types into a
  process it did not create.

---

## 1. Verification summary

| Provider | Version inspected | How it was verified |
| --- | --- | --- |
| Claude Code | `2.1.281` (installed in the research container) | `claude --help`, `claude agents --help`, `claude agents --json` executed; hooks reference and headless docs at `code.claude.com/docs/en/hooks` and `/headless`. **Phase 3:** hook payloads and stream-json lines captured from real runs (throw-away config folder), and one end-to-end run of the finished integration with the real CLI — see §3.5. |
| OpenAI Codex CLI | `0.156.1` (npm `@openai/codex@latest`) | CLI help of every subcommand; `codex features list` (`hooks = stable`); app-server protocol generated locally with `codex app-server generate-ts` / `generate-json-schema`; hooks config, runner and schemas read from the `rust-v0.156.1` source. **Phase 4:** real app-server and hook traffic recorded with a local fake model (no account) — see §4.5. |
| Cursor CLI (`agent`) | Not installable in the research container (download host blocked by the sandbox's network policy) | ACP schema from the official `@agentclientprotocol/sdk@1.5.0` package; Cursor docs (`cursor.com/docs/cli/acp`, `/docs/cli/reference/output-format`, `/docs/hooks`) via search snippets; community integration reports. **Every Cursor-specific claim below is marked "needs on-machine verification" until Phase 5 tests it on Windows.** |

---

## 2. Capability matrix

| Capability | Claude – managed | Claude – external | Codex – managed | Codex – external | Cursor – managed | Cursor – external |
| --- | --- | --- | --- | --- | --- | --- |
| Detect installation / version | Yes | Yes | Yes | Yes | Yes | Yes |
| Launch session | Yes (`claude -p` stream-json) | n/a | Yes (`codex app-server`, experimental upstream) | n/a | Yes (`agent acp`) | n/a |
| Observe (attach) | n/a | Yes (global hooks) | n/a | Partial (hooks need user trust) | n/a | Experimental (CLI fires a subset of hooks) |
| List sessions | Yes | Yes (`claude agents --json`) | Yes (`thread/list`, `thread/loaded/list`) | No (`codex agents` is TUI-only) | Runtime (`session/list` if advertised) | No |
| Stop | Yes (own process) | Partial (`claude stop <id>` for background sessions only) | Yes (`turn/interrupt`, own process) | No | Yes (`session/cancel`, own process) | No |
| Send prompt | Yes (stream-json on stdin) | No | Yes (`turn/start`, `turn/steer`) | No | Yes (`session/prompt`) | No |
| Structured events | Yes | Yes | Yes | Yes | Yes | Experimental |
| Tool events | Yes | Yes | Yes | Yes | Yes | Experimental |
| File events | Partial¹ | Partial¹ | Yes (`fileChange` items) | Partial¹ | Yes (`tool_call` kind `edit`/`read` + `locations`) | Experimental |
| Command events | Yes (`Bash`/`PowerShell` tool) | Yes | Yes (`commandExecution`, exit code, output deltas) | Yes | Yes (kind `execute`) | Experimental |
| Approve / reject permission from the app | Yes (`PermissionRequest` hook decision) | Partial² (opt-in) | Yes (server requests `*/requestApproval`) | Partial² (opt-in) | Yes (`session/request_permission`) | No |
| Subagents | Yes (`SubagentStart`/`SubagentStop`, `agent_id` on events) | Yes | Yes (`collabAgentToolCall`, `SubagentStart`/`Stop` hooks) | Yes (hooks) | No³ | No³ |
| Token usage | Yes (`result.modelUsage` in stream-json) | No | Yes (`thread/tokenUsage/updated`, summed over threads) | No | Runtime (`usage_update` if sent) | No |
| Cost | Partial⁴ (estimate) | No | No | No | Runtime | No |
| Model reported | Yes | Partial (`SessionStart.model`, not always) | Yes | Yes (hooks carry `model`) | Runtime | No |
| Context compaction | Yes (`PreCompact`/`PostCompact`) | Yes | Yes (`thread/compacted`) | Yes | Runtime (`compaction_update`) | Experimental |
| Resume / restart | Yes (`--resume <id>`) | n/a | Yes (`thread/resume`) | n/a | Runtime (`session/load`, `session/resume`) | n/a |

Notes:

1. **Tool-level file events.** Claude and Codex hooks report *what the agent asked a
   tool to do* (`tool_input.file_path` for `Edit`/`Write`/`Read`; the `apply_patch`
   envelope for Codex). This is evidence of intent + success, not a filesystem watch.
   The GitService provides the ground truth of what changed on disk. We never attribute
   a disk change to an agent without such tool-level evidence.
2. **External approvals are opt-in.** A `PermissionRequest` hook that waits for the app
   would hide the terminal prompt until the app answers. So by default the external
   hooks run in *observe* mode (the agent's own terminal prompt appears as usual and the
   office shows "waiting for permission"). The user can enable *"Answer permission
   prompts from Agent Office"*; then the hook waits (bounded timeout) and falls back to
   the terminal prompt if nobody answers.
3. **Cursor subagents.** ACP has no standard subagent concept. Cursor-specific
   extension methods (`cursor/task`, …) were reported by integrators but are not
   documented; community reports say `subagentStart/Stop` hooks do not fire in the CLI.
   Not implemented.
4. **Claude cost** — `total_cost_usd` is documented by Anthropic as a *client-side
   estimate*. The UI labels it "estimate reported by Claude Code", never as a bill.

---

## 3. Claude Code

### 3.1 Mechanisms

| Mechanism | Use in Agent Office | Stability |
| --- | --- | --- |
| **Hooks** (`hooks` key in `settings.json`) | External sessions (global `%USERPROFILE%\.claude\settings.json`) and managed sessions (per-process `--settings <json>`). | Documented, stable. |
| **Headless / Agent SDK CLI mode**: `claude -p --input-format stream-json --output-format stream-json --verbose` | Managed sessions: prompts in, typed messages out (`system/init`, `assistant`, `user`, `result`, `system/api_retry`, `system/permission_denied`). | Documented. |
| `--session-id <uuid>` | Managed sessions get an ID chosen by us, so hook events and the process are correlated from the first byte. | Documented. |
| `--resume <id>` | Restart a managed session. | Documented. |
| `claude agents --json` | List active interactive + background sessions (`pid`, `cwd`, `kind`, `startedAt`, `sessionId`, `name`, `status`). | In CLI help of 2.1.281; version-gated (checked at runtime). |
| `claude stop <id>` | Stop a *background* (`--bg`) session only. | In CLI help. |
| Transcript `.jsonl` files | **Not used.** Format is not a documented contract. | — |

### 3.2 Hook events (as documented today)

Events Agent Office subscribes to and how they map to the unified model:

| Claude hook | Key input fields | Unified event(s) |
| --- | --- | --- |
| `SessionStart` | `source` (`startup`/`resume`/`clear`/`compact`/`fork`), `model` (not always) | `session.started` / `session.updated` |
| `SessionEnd` | `reason` | `session.ended` |
| `UserPromptSubmit` | `prompt` | `prompt.submitted`, activity → thinking |
| `PreToolUse` | `tool_name`, `tool_input`, `tool_use_id` | `tool.started` (+ `command.started`, `file.read` intent …) |
| `PostToolUse` | `tool_response`, `tool_use_id` | `tool.completed` (+ `command.completed`, `file.modified`/`file.created`) |
| `PostToolUseFailure` | `tool_use_id` | `tool.failed` (+ `command.failed`) |
| `PermissionRequest` | `tool_name`, `tool_input`, `permission_suggestions` | `permission.requested` |
| `PermissionDenied` | tool fields | `permission.denied` |
| `Notification` | `notification_type` (`permission_prompt`, `idle_prompt`, …), `message` | `agent.waiting` / `agent.idle` |
| `Stop` | `last_assistant_message` | `agent.message` + `agent.idle` (external sessions; managed sessions take both from stream-json) |
| `StopFailure` | `error`, `error_details`, `last_assistant_message` | `agent.error` (external sessions) |
| `SubagentStart` | `agent_id`, `agent_type` | `subagent.started` |
| `SubagentStop` | `agent_id`, `agent_transcript_path` | `subagent.ended` |
| `PreCompact` / `PostCompact` | `trigger` | `context.compacted` |
| `CwdChanged` | `old_cwd`, `new_cwd` | `session.updated` (cwd) → GitService refresh |

`WorktreeCreate` is **not** subscribed: a command hook on that event *replaces*
Claude's own git worktree creation (documented), so observing it would change behaviour.

Common fields on every hook: `session_id`, `transcript_path`, `cwd`, `permission_mode`,
`hook_event_name`, and `agent_id` / `agent_type` when the event comes from a subagent.

`PermissionRequest` decision output (what our relay prints when the user answers in the app):

```json
{ "hookSpecificOutput": { "hookEventName": "PermissionRequest",
    "decision": { "behavior": "allow" } } }
```

`"behavior": "deny"` with `"message"` rejects. Printing nothing leaves the normal flow in place.

### 3.3 Windows specifics

* Hook commands default to Git Bash when installed, otherwise PowerShell. Agent Office
  uses the **exec form** (`command` + `args`) so no shell is involved at all: our
  hook command is the absolute path to `agent-office.exe` with the arguments
  `hook claude --origin global …`.
* Settings location: `%USERPROFILE%\.claude\settings.json`.
* Settings edits are picked up by Claude Code's file watcher; the user can inspect
  them with `/hooks`.
* In `-p` mode an unanswered permission prompt is denied unless a `PermissionRequest`
  hook allows it — this is exactly the documented path Agent Office uses to approve
  from the UI in managed sessions.

### 3.4 Limitations

* External sessions: no prompt injection, no token usage (hooks carry no usage).
* `claude agents --json` output is only described in CLI help; the adapter treats
  unknown/missing fields as absent and disables the feature if parsing fails.
* Hooks run only after the workspace-trust dialog was accepted (interactive mode).
* `PermissionRequest` carries no `tool_use_id`, so a permission cannot be tied to a
  specific tool call; Agent Office gives each request its own id and ties it to the
  session/agent.

### 3.5 Implementation status (Phase 3) and verified behaviour

Implemented in `ao-provider-claude`: detection, global hook integration
(install / repair / uninstall / status), external sessions through hooks, session
discovery with `claude agents --json`, managed sessions (`claude -p` stream-json +
per-session `--settings`), prompts to managed sessions, stop (own process tree;
external only background sessions via `claude stop <id>`), permission answers from
the app.

Settings integration (`%USERPROFILE%\.claude\settings.json`, or `%CLAUDE_CONFIG_DIR%`):

* the file is parsed first; invalid JSON or an unexpected `hooks` shape → status
  *Cannot read config*, **nothing is written**;
* only handlers whose command is `agent-office(.exe)` / `agent-office-hook(.exe)` with
  `claude` as provider argument are considered ours; everything else (user hooks,
  plugins, other settings) is preserved byte-for-byte in meaning;
* install is idempotent and removes duplicates; *Needs repair* when the executable
  moved or preferences changed the hook shape (changing preferences repairs it
  automatically);
* a timestamped backup `settings.json.agent-office-backup-<ms>` is written before
  every change (the last 5 are kept), then the new file is written to a temporary
  file and renamed over the original;
* `disableAllHooks: true` → *Needs your action* (we never flip the user's switch).

Observed with Claude Code 2.1.281 (fixtures in `fixtures/claude/`, real captures
marked `-real`):

| Finding | Consequence in Agent Office |
| --- | --- |
| Hooks from `--settings` are **merged** with user hooks, not replacing them. | Managed sessions ignore `global`-origin calls for sessions they launched. |
| Exec form (`command` + `args`), `async: true` and `timeout` (seconds) work as documented. | All observation hooks are async; only `PermissionRequest` (when answering) and `SessionEnd` are synchronous. |
| Async hooks are cancelled when a `-p` session tears down; `SessionEnd` has about 1.5 s. | `SessionEnd` is a quick synchronous hook (`--wait 3`, timeout 5 s). |
| `SubagentStart`/`SubagentStop` also fire for internal helper agents with an empty `agent_type`. | Those are ignored (no phantom subagents). |
| In `-p` mode `SessionStart` has no `model`; `system/init` has it. | Model of managed sessions comes from stream-json. |
| `result.usage` is per turn; `result.modelUsage` and `total_cost_usd` are running totals. | Usage = sum of `modelUsage`; cost always labelled *estimate*. |
| In `-p` mode both the `Stop` hook and the stream `result` report the end of a turn. | Managed sessions use stream-json only (no duplicate message/idle). |
| `--permission-mode` choices: `acceptEdits`, `auto`, `bypassPermissions`, `manual`, `dontAsk`, `plan`; `system/init` reports `manual` as `default`. | The New Agent dialog offers exactly these; `default` is sent as `manual`. |
| `--model` accepts aliases (`fable`, `opus`, `sonnet`) or full names. | Offered as suggestions; any name can be typed. |
| `claude agents --json` prints `pid`, `cwd`, `kind`, `startedAt`, `sessionId`, `name`, `status` (`busy`/`idle`). | Sessions appear before their first hook; coarse busy/idle is used only for sessions without hooks. |

Permission answers:

| Session | Agent Office setting | What happens |
| --- | --- | --- |
| Managed | always | The request waits for Approve / Reject in the app; no answer within the timeout → denied, with a message. |
| External | *Answer permission requests of external sessions* **off** (default) | Observe only: the request is shown; answer it in Claude's terminal. |
| External | on | The request waits for Approve / Reject in the app; no answer within the timeout → Claude shows its own prompt (`permission.expired`). |

---

## 4. OpenAI Codex CLI

### 4.1 Mechanisms

| Mechanism | Use in Agent Office | Stability |
| --- | --- | --- |
| **`codex app-server`** — JSON-RPC 2.0 over stdio (protocol v2) | Primary managed integration. Same protocol the Codex IDE extension/desktop app use. | Marked `[experimental]` in CLI help. The mapping follows the generated schema of the tested version (0.156.x) plus recorded traffic; other versions get a warning. |
| **Hooks** (`~/.codex/hooks.json`, or inline `[hooks]` in `config.toml`) | External sessions. | `hooks` feature is **stable** (`codex features list`). Requires **hook trust**: the user must trust the hook once via `/hooks` in the TUI; re-trust after the hook definition changes. |
| `codex exec --json` | Not used: app-server covers managed sessions on the tested version. (JSONL: `thread.started`, `turn.started`, `item.*`, `turn.completed`, `turn.failed`, `error`; no interactive approvals.) | Documented. |
| `hooks/list` (app-server) | Diagnostics: read our hook's `trustStatus` without guessing. | app-server API. |

### 4.2 app-server v2 surface used

* Client → server: `initialize` (+ `initialized` notification), `thread/start`
  (`cwd`, `model`, `approvalPolicy`), `turn/start` (`threadId`,
  `input: [{type: "text", text, text_elements: []}]`), `turn/steer` (while a
  turn runs), `turn/interrupt`, `hooks/list` (trust status).
* Server → client notifications mapped: `turn/started`, `turn/completed`
  (`completed` / `interrupted` / `failed` + `error`), `item/started`,
  `item/completed`, `thread/tokenUsage/updated`, `error` (`willRetry`),
  `thread/status/changed` (`systemError` only), `thread/closed`,
  `model/rerouted`, `thread/name/updated`, `serverRequest/resolved`.
  Deltas, hook runs, diffs, warnings, rate limits and realtime notifications
  are ignored.
* Server → client requests answered: `item/commandExecution/requestApproval`,
  `item/fileChange/requestApproval` (decisions `accept` / `acceptForSession` /
  `decline`), `item/permissions/requestApproval` (the requested profile is
  granted back, or nothing). Any other request (user input, MCP elicitation,
  auth refresh, …) gets a JSON-RPC error: Agent Office does not answer on the
  user's behalf.
* Thread items mapped: `userMessage`, `agentMessage`, `commandExecution`
  (command, cwd, exitCode, durationMs), `fileChange` (paths + add/update/delete),
  `mcpToolCall`, `dynamicToolCall`, `webSearch`, `imageView`,
  `collabAgentToolCall` (subagents), `contextCompaction`.

### 4.3 Hooks (external sessions)

Events (`HookEventName`): `SessionStart`, `SessionEnd`, `UserPromptSubmit`,
`PreToolUse`, `PermissionRequest`, `PostToolUse`, `PreCompact`, `PostCompact`,
`SubagentStart`, `SubagentStop`, `Stop`, `Interrupt`. There is no
`PostToolUseFailure` / `Notification` / `CwdChanged` equivalent.

Input is snake_case JSON on stdin (`session_id` = root thread id, `turn_id`,
`transcript_path`, `cwd`, `hook_event_name`, `model`, `permission_mode`, plus
`tool_name`, `tool_input`, `tool_use_id`, `tool_response`, `prompt`,
`agent_id`, `agent_type`, `source`, `reason`, `last_assistant_message`
depending on the event). Tool names are Codex's own: `Bash`
(`tool_input.command`; `tool_response` is the output text, no exit code),
`apply_patch` (`tool_input.command` is the patch envelope), `spawn_agent`,
`write_stdin`, `mcp__<server>__<tool>`, … `PermissionRequest` answers with
`hookSpecificOutput.decision.behavior = allow|deny` (+ `message`); the output
schema rejects unknown fields.

Config file `$CODEX_HOME/hooks.json` (default `~/.codex/hooks.json`):
`{"description"?, "hooks": {"<Event>": [{"matcher"?, "hooks": [handler]}]}}`,
unknown top-level keys make Codex reject the file. A command handler is
`{"type": "command", "command", "timeout", "async"}` and **`command` is a
shell command line** (`cmd.exe /C "<line>"` on Windows, `$SHELL -lc` elsewhere).

### 4.4 Limitations

* app-server is labelled experimental upstream: the mapping is pinned to what
  0.156.x sends, unknown messages are ignored, and a different version shows a
  warning in Diagnostics.
* External Codex sessions cannot be listed, stopped or prompted; they appear
  once a trusted hook fires. Hooks run only after the user trusts them in Codex
  (`/hooks`), and again after any change to them.
* No cost data; token usage only in managed sessions.
* On Windows Codex installs as an npm shim (`codex.cmd`) or a native
  `codex.exe`; both are started through the process manager (see ARCHITECTURE §6).

### 4.5 Implementation status (Phase 4) and verified behaviour

Implemented in `ao-provider-codex`: detection, managed sessions over
`codex app-server`, prompts (start + steer), graceful stop (interrupt, close
stdin, then end the process tree) and force stop, approvals from the app,
subagents, token usage, external sessions through hooks, install / repair /
uninstall / status of the hooks, and the trust check.

How it was verified: the real `codex-cli 0.156.1` was driven through
`app-server` and `codex exec` with a throw-away `CODEX_HOME` whose model
provider pointed at a **local fake Responses server** (no account, no network
calls to OpenAI, no cost). The recorded traffic is in
`fixtures/codex/*/*-real.json`; the finished adapter was then run against the
same real binary (managed session with approvals and a subagent, trust check,
external `codex exec` session through the hooks).

| Finding | Consequence in Agent Office |
| --- | --- |
| app-server messages omit `"jsonrpc"` and notifications carry `emittedAtMs`; server request ids start at 0. | The client requires neither field; `emittedAtMs` is used as the event time. |
| No `thread/started` is sent for a subagent thread; the `spawnAgent` collab item names it in `receiverThreadIds`, and its own notifications carry its `threadId`. | Child threads become subagents of the session (any unknown thread id too). |
| `thread/tokenUsage/updated.total` is a running total **per thread**. | Session usage = sum over the session's threads; cost is always "Unavailable". |
| A `commandExecution` item starts *before* its approval request; `fileChange` approvals only carry the item id. | Approvals are described from the item seen before ("Run: …", "Apply changes: add hello.txt"). |
| `commandActions` holds the parsed command, `command` includes the shell wrapper (`/bin/bash -lc '…'`). | The parsed command is shown when Codex parsed exactly one. |
| Without network Codex keeps retrying (`error` with `willRetry: true`). | Shown as recoverable errors; the user can stop the session. |
| User hooks also run inside app-server sessions; `session_id` equals the thread id. | Hook calls for managed threads are ignored (no duplicates, approvals stay in the protocol). |
| Hook trust is stored per key `<file>:<event>:<group>:<handler>` with a content hash. | Our groups are only appended; a removed group that is not the last stays as `{"hooks": []}` so the user's hooks keep their keys and trust. |
| `SessionEnd` hooks always run synchronously; `SessionEnd` and `Interrupt` timeouts are clamped to 3 s. | `SessionEnd` is a quick synchronous hook (`--wait 2`, timeout 3 s). |
| `hooks/list` works without an account and reports `trustStatus` / `enabled` per hook. | Diagnostics shows *Needs your action* until the hooks are trusted in Codex; Agent Office never writes trust state. |
| `codex --help` documents approval policies `on-request` and `never`; the protocol also accepts `untrusted` (asks before commands Codex does not consider safe). | New agent offers these three; nothing is sent when left at "Use my Codex settings". |

Approval answers:

| Session | Agent Office setting | What happens |
| --- | --- | --- |
| Managed | always | Codex waits for Approve / Reject in the app; no answer within the timeout → declined. |
| External | *Answer permission requests of external sessions* **off** (default) | Observe only: the request is shown; answer it in Codex. |
| External | on | The `PermissionRequest` hook waits for Approve / Reject in the app; no answer in time → Codex asks in its own UI. |

---

## 5. Cursor CLI (`agent`, formerly `cursor-agent`)

### 5.1 Mechanisms

| Mechanism | Use in Agent Office | Stability |
| --- | --- | --- |
| **`agent acp`** — Agent Client Protocol, JSON-RPC 2.0 over stdio | Primary managed integration. | Officially documented by Cursor (`/docs/cli/acp`). |
| `agent -p --output-format stream-json` | One-shot fallback (`system/init`, `assistant`, `tool_call` started/completed, `result`). | Documented. |
| Hooks (`~/.cursor/hooks.json`, `.cursor/hooks.json`, `version: 1`) | External sessions — **experimental**. Docs list `sessionStart`, `sessionEnd`, `preToolUse`, `postToolUse`, `postToolUseFailure`, `subagentStart`, `subagentStop`, `beforeShellExecution`, `afterShellExecution`, `beforeMCPExecution`, `afterMCPExecution`, `beforeReadFile`, `afterFileEdit`, `beforeSubmitPrompt`, `preCompact`, `stop`, `afterAgentResponse`, `afterAgentThought`. Community reports say the CLI fires only a subset. | Diagnostics records which events were actually observed per machine. |

### 5.2 ACP surface used

* Client → agent: `initialize` (protocol version + client capabilities; we declare
  **no** `fs` / `terminal` client capabilities in the vertical slice, so the agent uses
  its own tools), `authenticate` (Cursor advertises the `cursor_login` method; the user
  logs in once with `agent login`), `session/new` (`cwd`, `mcpServers: []`),
  `session/load` / `session/resume` (if `agentCapabilities` advertises them),
  `session/prompt`, `session/cancel`, `session/set_mode`.
* Agent → client notification `session/update` with `sessionUpdate` kinds:
  `user_message_chunk`, `agent_message_chunk`, `agent_thought_chunk`, `tool_call`,
  `tool_call_update`, `plan`, `current_mode_update`, `session_info_update`,
  `usage_update`, `compaction_update`, `notice`, …
* Tool kinds: `read`, `edit`, `delete`, `move`, `search`, `execute`, `think`,
  `fetch`, `switch_mode`, `other`; status `pending`/`in_progress`/`completed`/`failed`.
* Agent → client request `session/request_permission` with `options[]`, each with an
  opaque `optionId` and a `kind` (`allow_once`, `allow_always`, `reject_once`,
  `reject_always`). **We always answer by echoing the `optionId` of the option whose
  `kind` matches the user's choice** — Cursor's ids are hyphenated (`allow-once`), so
  hard-coding ids breaks.
* `session/prompt` resolves with a `stopReason`: `end_turn`, `max_tokens`,
  `max_turn_requests`, `refusal`, `cancelled`.

### 5.3 Known quirks (from integrator reports, to verify in Phase 5)

* Cursor-specific extension requests (`cursor/ask_question`, `cursor/create_plan`,
  `cursor/task`, `cursor/update_todos`) may be sent; an unknown request **must** get a
  JSON-RPC "method not found" error immediately, otherwise the turn stalls.
* If the client never answers `session/request_permission`, tool execution blocks —
  the adapter always answers (user decision, timeout → `reject_once`).

### 5.4 Windows

Cursor ships an official native PowerShell installer
(`irm 'https://cursor.com/install?win32=true' | iex`). Older docs required WSL; native
Windows support is recent, so Phase 5 must confirm executable name/location
(`agent.exe`, `cursor-agent`, `%LOCALAPPDATA%`) on a real Windows 11 machine.

---

## 6. Cross-provider normalization rules

| Normalized tool category | Claude tool names | Codex item types / tools | ACP `ToolKind` |
| --- | --- | --- | --- |
| `read` | `Read`, `NotebookRead` | `commandExecution` with `read` action | `read` |
| `search` | `Grep`, `Glob`, `LS` | `commandExecution` with `search`/`listFiles` actions | `search` |
| `edit` | `Edit`, `MultiEdit`, `Write`, `NotebookEdit` | `fileChange`, `apply_patch` | `edit`, `delete`, `move` |
| `execute` | `Bash`, `PowerShell` | `commandExecution` | `execute` |
| `fetch` | `WebFetch`, `WebSearch` | `webSearch` | `fetch` |
| `subagent` | `Task`, `Agent` | `collabAgentToolCall` | — |
| `mcp` | `mcp__*` | `mcpToolCall` | — |
| `think` | — | `reasoning` | `think` |
| `other` | anything else | `dynamicToolCall` | `other`, `switch_mode` |

The visual activity (`CODING`, `READING`, `RUNNING_COMMAND`, `TESTING`, …) is derived
from the category. `TESTING` is a *display heuristic* (command matches a test runner
pattern such as `npm test`, `cargo test`, `pytest`); it never changes stored data.
