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
| Claude Code | `2.1.281` (installed in the research container) | `claude --help`, `claude agents --help`, `claude agents --json` executed; hooks reference and headless docs at `code.claude.com/docs/en/hooks` and `/headless`. |
| OpenAI Codex CLI | `0.156.1` (npm `@openai/codex@latest`) | CLI help of every subcommand; `codex features list` (`hooks = stable`); app-server protocol generated locally with `codex app-server generate-ts` / `generate-json-schema`; hooks input/output schema read from `openai/codex` source (`codex-rs/hooks/src/schema.rs`). |
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
| Token usage | Yes (`result.usage` in stream-json) | No | Yes (`thread/tokenUsage/updated`) | No | Runtime (`usage_update` if sent) | No |
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
| `Stop` | `last_assistant_message` | `agent.idle` (turn done) |
| `StopFailure` | `error_type` | `agent.error` |
| `SubagentStart` | `agent_id`, `agent_type` | `subagent.started` |
| `SubagentStop` | `agent_id`, `agent_transcript_path` | `subagent.ended` |
| `PreCompact` / `PostCompact` | `trigger` | `context.compacted` |
| `CwdChanged`, `WorktreeCreate`, `WorktreeRemove` | — | `session.updated` (cwd/worktree) → GitService refresh |

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
  hook command is the absolute path to `agent-office-hook.exe`.
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

---

## 4. OpenAI Codex CLI

### 4.1 Mechanisms

| Mechanism | Use in Agent Office | Stability |
| --- | --- | --- |
| **`codex app-server`** — JSON-RPC 2.0 over stdio (protocol v2) | Primary managed integration. Same protocol the Codex IDE extension/desktop app use. | Marked `[experimental]` in CLI help. We pin tested versions, generate types from `generate-json-schema`, and degrade to `exec --json` if `initialize` fails. |
| **Hooks** (`~/.codex/hooks.json`, or inline `[hooks]` in `config.toml`) | External sessions. | `hooks` feature is **stable** (`codex features list`). Requires **hook trust**: the user must trust the hook once via `/hooks` in the TUI; re-trust after the hook definition changes. |
| `codex exec --json` | One-shot tasks fallback (JSONL: `thread.started`, `turn.started`, `item.*`, `turn.completed` with usage, `turn.failed`, `error`). | Documented. No interactive approvals. |
| `hooks/list` (app-server) | Diagnostics: read our hook's `trustStatus` without guessing. | app-server API. |

### 4.2 app-server v2 surface used

* Client → server: `initialize` (+ `initialized` notification), `thread/start`
  (`cwd`, `model`, `approvalPolicy`, `sandbox`), `thread/resume`, `thread/list`,
  `thread/loaded/list`, `turn/start` (`threadId`, `input: [{type:"text", text}]`),
  `turn/steer`, `turn/interrupt`, `hooks/list`, `model/list`.
* Server → client notifications: `thread/started`, `thread/status/changed`
  (`idle` / `active{waitingOnApproval|waitingOnUserInput}` / `systemError`),
  `turn/started`, `turn/completed` (`completed`/`interrupted`/`failed`),
  `item/started`, `item/completed`, `item/commandExecution/outputDelta`,
  `item/agentMessage/delta`, `thread/tokenUsage/updated`
  (`inputTokens`, `cachedInputTokens`, `outputTokens`, `reasoningOutputTokens`, `totalTokens`),
  `thread/compacted`, `hook/started`, `hook/completed`, `error`.
* Server → client requests (approvals): `item/commandExecution/requestApproval`,
  `item/fileChange/requestApproval`, `item/permissions/requestApproval`;
  decisions `accept` / `acceptForSession` / `decline` / `cancel`.
* Thread items mapped: `userMessage`, `agentMessage`, `reasoning`, `plan`,
  `commandExecution` (command, cwd, exitCode, durationMs), `fileChange` (paths),
  `mcpToolCall`, `dynamicToolCall`, `collabAgentToolCall` (subagents: `spawnAgent`,
  `receiverThreadIds`), `webSearch`.

### 4.3 Hooks (external sessions)

Events (`HookEventName` in the protocol): `SessionStart`, `SessionEnd`,
`UserPromptSubmit`, `PreToolUse`, `PermissionRequest`, `PostToolUse`, `PreCompact`,
`PostCompact`, `SubagentStart`, `SubagentStop`, `Stop`, `Interrupt`.
Input is snake_case JSON on stdin with `session_id`, `turn_id`, `transcript_path`,
`cwd`, `hook_event_name`, `model`, `permission_mode`, plus `tool_name`, `tool_input`,
`tool_use_id`, `tool_response`, `prompt`, `agent_id`, `agent_type`, `source`,
`reason`, `last_assistant_message` depending on the event. Output is camelCase JSON;
`PermissionRequest` answers with `hookSpecificOutput.decision.behavior = allow|deny`.
Hook config shape is the same as Claude's (`{"hooks": {"PreToolUse": [{"matcher": …, "hooks": [{"type": "command", …}]}]}}`).

### 4.4 Limitations

* app-server is labelled experimental upstream → version pinning + contract tests.
* External Codex sessions cannot be listed or controlled; they appear only once a
  trusted hook fires.
* No cost data; token usage only in managed sessions.
* On Windows Codex installs as an npm shim (`codex.cmd`) or a native `codex.exe`;
  `.cmd` shims must be launched through `cmd.exe /d /s /c` — handled by the process manager.

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
