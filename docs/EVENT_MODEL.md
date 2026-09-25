# Unified Event Model

Every provider adapter translates its native signals (hooks, JSON-RPC
notifications, stream-json lines) into `AgentEvent`s. Everything downstream —
state, database, UI — only understands this model. Rust definitions live in
`crates/ao-core/src/event.rs`; TypeScript types are generated into `src/bindings/`.

## Envelope

```json
{
  "eventId": "7b0c…",            // UUID generated at ingestion
  "timestamp": 1790236673845,     // ms since epoch, when the provider/relay observed it
  "provider": "claude",           // [a-z0-9_-], ≤ 64 chars
  "sessionId": "11111111-…",      // provider session id
  "agentId": "11111111-…",        // = sessionId for the main agent
  "parentAgentId": "…",           // subagents only (optional)
  "projectId": "…",               // resolved from cwd by the pipeline (optional)
  "repositoryId": "…",            // set by the Git service: the repository's main folder (optional)
  "source": "hook",               // hook | protocol | process | git | internal | simulation
  "type": "tool.started",
  "payload": { … }
}
```

## Event types

| Type | Payload | Effect on the office |
| --- | --- | --- |
| `session.started` | `mode`, `cwd`, `model`, `title`, `reason`, `permissionMode`, `worktree`, `pid` | Creates the session and its main character; re-opens an ended session (resume) |
| `session.updated` | same as above (only present fields change) | Updates session metadata |
| `session.ended` | `reason`, `exitCode` | Every agent → `DONE`, then they walk out |
| `prompt.submitted` | `text` (omitted if prompt storage is off) | `THINKING` |
| `agent.thinking` | `text` | `THINKING` |
| `agent.message` | `text` | Stored as last message (panel) |
| `agent.idle` | `text` | `IDLE` (lounge); clears running tools and pending permission |
| `agent.waiting` | `reason` (`permission`/`input`/`other`), `message` | `WAITING_PERMISSION` / `WAITING_INPUT` / `IDLE` |
| `agent.error` | `message`, `errorType`, `recoverable` | `ERROR` |
| `tool.started` | `toolCallId`, `toolName`, `category`, `title` | Activity from category (see below) |
| `tool.completed` / `tool.failed` | `toolCallId`, `toolName`, `category`, `durationMs`, `detail` | Resume outer tool or `THINKING` |
| `file.read` | `path`, `toolCallId` | Log only |
| `file.created` / `file.modified` / `file.deleted` | `path`, `toolCallId` | Adds to "files changed (tool evidence)" |
| `command.started` | `commandId`, `command`, `cwd` | `RUNNING_COMMAND` or `TESTING` |
| `command.output` | `commandId`, `stream`, `chunk` | Log only |
| `command.completed` / `command.failed` | `commandId`, `command`, `exitCode`, `durationMs`, `error` | Settle activity |
| `permission.requested` | `requestId`, `toolName`, `description`, `canResolve`, `options[]` | `WAITING_PERMISSION`; Approve/Reject shown only if `canResolve` |
| `permission.approved` / `permission.denied` | `requestId`, `resolvedBy`, `message` | Clears the pending request |
| `permission.expired` | `requestId`, `resolvedBy: timeout`, `message` | Agent Office stopped offering an answer; the agent keeps waiting in its own prompt |
| `subagent.started` / `subagent.updated` / `subagent.ended` | `agentType`, `description`, `reason` | New character linked to its parent / walks out |
| `git.branch_changed` | `branch` (absent when detached), `previous` | Session branch |
| `git.status_changed` | `dirty` (not staged: edits, new files, conflicts), `staged`, `untracked`, `conflicted`, `ahead`, `behind` (only with an upstream) | Session Git changes |
| `git.commit_created` | `sha`, `summary` | Session commit count; only sent to a session that ran the commit (see below) |
| `context.compacted` | `text` | `THINKING` ("Compacting context") |
| `usage.updated` | `inputTokens`, `outputTokens`, `cachedInputTokens`, `reasoningTokens`, `totalTokens`, `contextWindow`, `contextTokens` (tokens currently in the context, ACP), `costUsd`, `costIsEstimate` | Session usage (cumulative, only reported values) |
| `provider.error` | `component`, `message` | Diagnostics; last error on the agent if the session exists |

### Activities

| Activity | From |
| --- | --- |
| `IDLE` | session start, `agent.idle` |
| `THINKING` | prompt, thinking, tool finished with nothing else running, compaction |
| `READING` | tool category `read`, `search`, `fetch` |
| `CODING` | tool category `edit` |
| `RUNNING_COMMAND` | category `execute`, `command.started` |
| `TESTING` | `execute`/command that looks like a test runner (display heuristic only) |
| `WAITING_PERMISSION` | `permission.requested`, `agent.waiting(permission)` |
| `WAITING_INPUT` | `agent.waiting(input)` (the agent asked the user a question) |
| `ERROR` | `agent.error` |
| `DONE` | `session.ended`, `subagent.ended` |

## Pipeline guarantees

Every event goes through `ao_core::pipeline::Pipeline::ingest` before it
touches state, storage or the UI:

1. **Validation** (`AgentEvent::validate`): provider id format, non-empty ids,
   no self-parenting, subagent events must target a subagent, tool/file events
   need names/paths. Invalid events are **rejected and reported** in
   Diagnostics (`events rejected`, provider errors) — never applied.
2. **Clock sanity**: timestamps more than 5 min in the future or before 2000
   (seconds sent instead of ms) are re-stamped with the ingestion time.
3. **De-duplication** by `(provider, session, type, correlation id)` where the
   correlation id is the tool call / permission request / command id.
4. **Sanitization**: secrets are redacted and every string is capped
   (default 8 KB, configurable); prompt text can be dropped entirely.
5. **Project resolution** from the session's working folder.

## Ordering tolerance

Hooks of some providers run asynchronously, so two events of the same agent can
arrive out of order. The reducer is written to converge anyway:

* A `tool.completed` that arrives before its `tool.started` is remembered, and
  the late start is counted but never shown as running.
* A pending permission is cleared by later work of the same agent
  (`tool.*`, `command.completed/failed`, `prompt.submitted` with a newer
  timestamp): that proves the request was answered elsewhere, e.g. in the
  terminal. Older, late-delivered events do not clear it.
* `subagent.ended`, `agent.idle` and `agent.message` never create an unknown
  subagent (Claude also fires `SubagentStop` for internal helper agents).

## Batching and performance

The host flushes changed sessions/agents plus at most 256 raw events per batch
every 100 ms. Load tests: 10,000 events through pipeline + reducer + batcher in
well under a second (`crates/ao-core/tests/load.rs`), and the UI store absorbs
40 batches / 10,000 events in under a second (`src/state/perf.test.ts`).

## Testing adapters

Provider mappings are tested with fixture files (see `crates/ao-testkit`):
`fixtures/<provider>/<channel>/*.json` hold a real-shaped input and the
expected events as a subset match, ignoring `eventId` and `timestamp`.

## Git events

Git events come from Agent Office's own Git service (`source: git`), not from
the providers. They describe the working tree a session works in, so they
never count as agent activity: they do not end a "silent" warning and never
create a session.

* `git.branch_changed` and `git.status_changed` are facts about the working
  tree; every active session in that tree receives them.
* `git.commit_created` is a claim that *this session* made the commit. It is
  sent only with evidence: the session ran a commit-creating `git` command
  (`commit`, `merge`, `cherry-pick`, `revert`, `am`, `rebase`, `pull`) in that
  tree, and the commit's time falls within that command's run. When no
  session, or more than one, has such evidence, the commit is shown in the
  repository and credited to nobody.
* A linked worktree is reported with `session.updated` (`worktree`).

