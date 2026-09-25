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
| Cursor CLI (`agent`) | Not installable in the research container: `cursor.com` (docs and downloads) is blocked by the sandbox's network policy, in Phase 0 and again in Phase 5 | ACP schema from the official `@agentclientprotocol/sdk@1.5.0` package; Cursor docs via search snippets; integrator reports (OpenHands, Hermes). **Phase 5:** the adapter was run against the **official ACP example agent** (an independent implementation) and all traffic of the end-to-end tests was validated against the official schema — see §5.5. **Every Cursor-specific claim is marked "needs on-machine verification"; the checklist is in ROADMAP §8.** |

---

## 2. Capability matrix

| Capability | Claude – managed | Claude – external | Codex – managed | Codex – external | Cursor – managed | Cursor – external |
| --- | --- | --- | --- | --- | --- | --- |
| Detect installation / version | Yes | Yes | Yes | Yes | Yes | Yes |
| Launch session | Yes (`claude -p` stream-json) | n/a | Yes (`codex app-server`, experimental upstream) | n/a | Experimental (`agent acp`; implemented from the ACP spec, not yet run against a real Cursor) | n/a |
| Observe (attach) | n/a | Yes (global hooks) | n/a | Partial (hooks need user trust) | n/a | Experimental upstream (CLI fires a subset of hooks); **not implemented** |
| List sessions | Yes | Yes (`claude agents --json`) | Yes (`thread/list`, `thread/loaded/list`) | No (`codex agents` is TUI-only) | No (ACP `session/list` exists; not used yet) | No |
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
| Resume / restart | Yes (`--resume <id>`) | n/a | Yes (`thread/resume`) | n/a | No (ACP `session/load` exists; not used yet) | n/a |

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
| `--resume <id>` | Restart a managed session (Agent Office → *Restart*): same session id, same folder, model and permission mode. | Documented. With 2.1.282, an unknown id prints `No conversation found with session ID: <id>` on stderr and a `result` line (`subtype: error_during_execution`, `is_error: true`, `errors: [...]`) on stdout, then exits 1 (recorded without an API call: `fixtures/claude/stream/08-resume-unknown-session-real.json`). |
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
  (`cwd`, `model`, `approvalPolicy`), `thread/resume` (`threadId`,
  `excludeTurns: true`; used by *Restart*), `turn/start` (`threadId`,
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
| `thread/resume` keeps the thread id, returns no turns with `excludeTurns`, and sends no `thread/started` (only `warning`, `thread/status/changed` idle and `thread/goal/cleared`). An unknown id fails with `-32600` "no rollout found for thread id …". Verified with threads saved by earlier local runs (no network, no cost). | *Restart* announces the session itself, checks that the returned id is the one asked for, and shows Codex's error message when the thread is gone. |
| `codex --help` documents approval policies `on-request` and `never`; the protocol also accepts `untrusted` (asks before commands Codex does not consider safe). | New agent offers these three; nothing is sent when left at "Use my Codex settings". |

Approval answers:

| Session | Agent Office setting | What happens |
| --- | --- | --- |
| Managed | always | Codex waits for Approve / Reject in the app; no answer within the timeout → declined. |
| External | *Answer permission requests of external sessions* **off** (default) | Observe only: the request is shown; answer it in Codex. |
| External | on | The `PermissionRequest` hook waits for Approve / Reject in the app; no answer in time → Codex asks in its own UI. |

Switching this setting (or the wait time while it is on) rewrites the
`PermissionRequest` hook, so Codex marks that one hook as modified and asks the
user to trust it again (`/hooks`); Diagnostics shows *Needs your action* until then.

---

## 5. Cursor CLI (`agent`, formerly `cursor-agent`)

### 5.1 Mechanisms

| Mechanism | Use in Agent Office | Stability |
| --- | --- | --- |
| **`agent acp`** — Agent Client Protocol, JSON-RPC 2.0 over stdio | Primary managed integration. | Officially documented by Cursor (`/docs/cli/acp`). |
| `agent -p --output-format stream-json` | Not used (ACP covers managed sessions); a possible one-shot fallback. | Documented. |
| Hooks (`~/.cursor/hooks.json`, `.cursor/hooks.json`, `version: 1`) | External sessions — **not implemented** (§5.5). Docs list `sessionStart`, `sessionEnd`, `preToolUse`, `postToolUse`, `postToolUseFailure`, `subagentStart`, `subagentStop`, `beforeShellExecution`, `afterShellExecution`, `beforeMCPExecution`, `afterMCPExecution`, `beforeReadFile`, `afterFileEdit`, `beforeSubmitPrompt`, `preCompact`, `stop`, `afterAgentResponse`, `afterAgentThought`. Community reports say the CLI fires only a subset. | Payloads not verifiable in the build environment. |

### 5.2 ACP surface used

Implemented in `ao-provider-cursor` (`src/acp.rs` maps messages, `src/lib.rs`
drives the session) from the official schema of `@agentclientprotocol/sdk` 1.5.0,
protocol version 1:

* Client → agent: `initialize` (protocol version 1; **no** `fs` / `terminal`
  client capabilities, so the agent keeps using its own tools; `clientInfo`
  `agent-office`), `authenticate` (see §5.3), `session/new` (`cwd`,
  `mcpServers: []`), `session/set_config_option` (model), `session/set_model`
  (older model selection, see §5.3), `session/set_mode`, `session/prompt`
  (text only) and the `session/cancel` notification.
* Agent → client notification `session/update`:

  | `sessionUpdate` | Unified event |
  | --- | --- |
  | `agent_message_chunk` | collected, one `agent.message` per text run (flushed before a tool starts and at the end of the turn) |
  | `agent_thought_chunk` | `agent.thinking` ("Thinking"), once per run |
  | `tool_call` / `tool_call_update` | `tool.started` / `tool.completed` / `tool.failed`; kind `execute` also `command.*` (command from `rawInput.command`, else the title; ACP has no exit code); completed `edit` → `file.created` (diff without `oldText`) or `file.modified`; `delete` → `file.deleted`; `read` → `file.read` |
  | `plan` | `agent.thinking` "Plan: n/m steps done" |
  | `current_mode_update`, `config_option_update` (model), `session_info_update` (title) | `session.updated` |
  | `usage_update` (`used`, `size`, `cost`) | `usage.updated` (context tokens / window; cost only when the currency is USD) |
  | `compaction_update` | "Compacting context", then `context.compacted` (or an error) |
  | `notice` with severity `error` | recoverable `agent.error` |
  | `user_message_chunk`, `available_commands_update`, unknown kinds | ignored |

* Agent → client request `session/request_permission` → `permission.requested`
  with the agent's options. **The answer echoes the `optionId` of the option
  whose `kind` matches the decision** (approve → `allow_once`, approve for the
  session → `allow_always`, reject → `reject_once`, each falling back to the
  other variant of the same decision); ids are never hard-coded. An unanswered
  request is answered with `reject_once` after the configured wait; stopping a
  turn answers pending requests with `cancelled`, as ACP requires.
* Any other agent → client request is answered **at once** with JSON-RPC
  `-32601` ("method not found") and shown as a recoverable error on the agent,
  so the turn never stalls.
* `session/prompt` result: `stopReason` `end_turn` → idle; `cancelled` → idle
  "Turn cancelled"; `max_tokens`, `max_turn_requests`, `refusal` → recoverable
  error. `usage` (input/output/total/thought/cached tokens), when present, becomes
  `usage.updated`. Tool calls still open at the end of a turn close as failed when
  their outcome is known (cancelled, not allowed, turn stopped) and are otherwise
  dropped without inventing a result.

### 5.3 Cursor specifics (integrator reports — to verify on a real Cursor)

| Report | How Agent Office handles it |
| --- | --- |
| `agent acp` starts the ACP server (executable `agent`, older name `cursor-agent`). | Detection tries `cursor-agent` first (`agent` is a generic name), then `agent`, also in `%LOCALAPPDATA%\cursor-agent`. |
| `initialize` offers only the auth method `cursor_login` and no `agentInfo.name`; sessions stay unusable without an explicit `authenticate`. It answers at once when the user already ran `agent login`. | When `cursor_login` is offered, `authenticate` is called before `session/new` (wait up to 180 s). If login fails the launch error says to run `agent login`. Other agents authenticate only when `session/new` fails. `CURSOR_API_KEY` in the environment is Cursor's documented headless alternative; Agent Office never reads or stores it. |
| `session/new` returns modes `agent` / `plan` / `ask`, config options for mode and model, and also the older `models` state (`currentModelId` `default[]` = Auto, ~36 models). | A requested model is looked up in the `model` config option (`session/set_config_option`) or, if only `models` exists, set with the older `session/set_model` (not in the 1.5.0 schema). Exact id or name first, then the base of ids with options (`composer-2.5` → `composer-2.5[fast=true]`). A model or mode that is not offered keeps Cursor's default and shows a warning on the agent. |
| `session/set_mode` accepts `agent` and `ask`. | A mode is only sent when the agent announces it. The New agent dialog offers agent / plan / ask as hints. |
| Permission option ids are hyphenated (`allow-once`), unlike other agents (`allow_once`). | Options are chosen by `kind`; ids are echoed. |
| Extension requests `cursor/ask_question`, `cursor/create_plan`, `cursor/task`, `cursor/update_todos`, `cursor/generate_image` block the turn until answered. | Declined at once (`-32601`) and shown as "Cursor sent `cursor/…`, which Agent Office cannot handle yet". Their parameters are undocumented, so they are not interpreted. |
| Login state lives in `~/.cursor`. | Agent Office never reads or writes it. |

### 5.4 Windows

Cursor ships an official native PowerShell installer
(`irm 'https://cursor.com/install?win32=true' | iex`). Older docs required WSL; native
Windows support is recent. `.cmd` / `.ps1` shims are handled by detection and process
spawning as for Codex, but the executable name and location on Windows 11 must be
confirmed on a real machine (ROADMAP §8).

### 5.5 Implementation status (Phase 5) and how it was verified

Implemented: detection, managed sessions over `agent acp` (launch, prompts one
turn at a time, model and mode selection, permissions from the app with timeout,
graceful stop = `session/cancel` + cancelled answers + close stdin, force stop =
process tree), runtime usage / cost / model / compaction.

Not implemented:

* **External sessions (Cursor hooks).** Cursor documents a `hooks.json` with
  `version: 1`, but its payload shapes, the events the CLI actually fires (reports
  say only a subset) and how blocking hooks answer could not be read or tested:
  the docs and the CLI download are blocked in the build environment. A mapping
  written from guesses would be the fragile parsing the project rules forbid, so
  Agent Office does **not** touch `~/.cursor/hooks.json` and shows no terminal
  Cursor sessions. Cursor sessions launched from Agent Office are fully visible.
* Subagents (no ACP concept), session listing and resume (`session/list`,
  `session/load` exist in ACP but are optional for agents and could not be
  tested against Cursor, so *Restart* is refused for Cursor sessions with a
  clear message), images in prompts, MCP
  servers passed to the agent, Cursor's plan / question / todo extension requests.

Verification done without a Cursor account or network:

| Check | Result |
| --- | --- |
| Official ACP example agent (`@agentclientprotocol/sdk` 1.5.0 `dist/examples/agent.js`) driven by the adapter: approve, reject, cancel mid-turn, graceful stop | All passed. It showed that agents **reuse tool call ids across turns** (`call_1` every turn) and may leave a rejected tool without a final update; both are now handled (fixtures `fixtures/cursor/acp/*-sdk.json` keep its messages). |
| Every message of the end-to-end tests (both directions: Agent Office requests and answers, agent responses and notifications) validated with the SDK's own zod schemas | 120 messages, 0 invalid. Not covered by the schema: `session/set_model` (older API) and `cursor/*` extension requests. |
| `fake-cursor` (ACP per schema + the reports above) through the real host (`src-tauri/tests/cursor_e2e.rs`) | Streaming, usage, model/mode, approve / approve-for-session / reject, file events, commands, cancel with a pending permission, cancel of a long turn, eager login, failed login, older model selection, declined extension requests, prompt errors, unknown model / mode warnings. |

What only a real Cursor on Windows 11 can confirm is listed in ROADMAP §8.

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
