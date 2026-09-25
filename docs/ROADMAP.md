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
| **4 – Codex adapter** | app-server JSON-RPC client, thread/turn lifecycle, approvals, token usage, subagents; external hooks in `~/.codex/hooks.json` + trust status via `hooks/list` | Protocol tests against a fake app-server replaying recorded JSON-RPC; version pin checks |
| **5 – Cursor adapter** | ACP client (initialize/authenticate/session/new/prompt/cancel/request_permission, unknown-method handling), runtime capabilities; hooks as experimental; on-machine verification on Windows | ACP tests with a fake agent; manual verification checklist executed on Windows 11 |
| **6 – Office visualization** | Pixel sprites + animations per activity, zones, subagent spawn/leave, badges, selection, performance budget | 20 sessions + 50 subagents at 60 fps on a mid-range laptop |
| **7 – Process management** | `ao-process`: Job Objects, graceful stop, force kill, exit/stall/restart detection, `.cmd` shim handling, PID-reuse guard | Process tests on Windows CI with a fake agent (tree kill, no foreign kill) |
| **8 – Git integration** | `ao-git` (repo, branch, worktree, dirty/staged, ahead/behind, recent commits), refresh on tool/cwd events, conservative attribution | Tests on temporary repositories |
| **9 – Diagnostics & notifications** | Full diagnostics screen, observed-hook evidence, log viewer, Windows toast notifications with click-to-open | Diagnostics report exportable; notification click opens the session |
| **10 – Packaging** | NSIS `AgentOfficeSetup.exe` (Start Menu, desktop shortcut, uninstaller that first removes Agent Office hooks from provider settings), WebView2 bootstrapper, version metadata, optional signing hook, updater plumbing | Clean-VM install/uninstall smoke test |

## 3. Technical risks

| # | Risk | Impact | Mitigation |
| --- | --- | --- | --- |
| R1 | **Codex app-server is marked experimental** and its protocol changes often (0.156.x). | Managed Codex sessions break after a Codex update. | Mapping pinned to the tested 0.156.x (generated schema + recorded traffic), unknown messages ignored, other versions get a warning in Diagnostics, fixtures can be re-recorded without an account (DEVELOPMENT.md). |
| R2 | **Cursor CLI could not be executed during research or Phase 5** (`cursor.com` blocked in the build environment); native Windows support is recent; hook coverage in the CLI is reported incomplete. | Cursor features may differ on real machines. | ACP implemented from the official schema, tested against the official example agent and schema-validated; values the agent announces at runtime are used as announced; launch marked *Experimental*; hooks not implemented; manual Windows 11 checklist in §8. |
| R3 | **Codex hook trust** requires a manual `/hooks` confirmation by the user, again after any change to the hook. | External Codex sessions stay invisible until trusted. | Implemented: status read from Codex (`hooks/list` `trustStatus`), *Needs your action* with the exact step in Diagnostics; our entries are only appended so the user's hooks keep their trust; trust is never written by Agent Office. |
| R4 | Editing `~/.claude/settings.json` could damage user config. | Lost settings/plugins. | Parse-or-refuse, timestamped backup, atomic write (temp + rename), only touch entries carrying our marker, idempotent install, uninstall/repair, tests with real-world settings fixtures. |
| R5 | A synchronous permission hook would hide the terminal prompt. | User confusion, stuck agents. | Observe-only by default; opt-in "answer from app" with bounded timeout and fallback to the terminal prompt. |
| R6 | Windows process trees (`.cmd` shims, `node` children, shells spawned by agents). | Orphans or wrong process killed. | Implemented: one Job Object per managed process, started suspended so nothing escapes it; only our jobs are ever terminated; the process handle is held for its whole life (no action on a bare PID); leftovers stopped when the agent exits; tested on Windows CI. |
| R7 | Hook latency slows the agent. | Worse agent UX. | Relay is the native app exe in `hook` mode (≈25 ms per call in a debug build when the app is closed), async hooks for non-decision events, 300 ms connect timeout, exit 0 when the app is closed. |
| R13 | App uninstalled while hooks remain in `~/.claude/settings.json` or `~/.codex/hooks.json`. | The agent reports a failing hook command on every event. | Phase 10: the NSIS uninstaller runs the hook removal first; until then Diagnostics → *Uninstall* must be used before removing the app (documented in the README). |
| R8 | Event floods (streaming deltas, command output). | UI jank, DB growth. | Batching (100 ms), UI never re-renders per token, output truncation, retention, virtualized lists. |
| R9 | Windows toast click-activation is limited in the Tauri notification plugin on desktop. | Click-to-open may not work in MVS. | Use WinRT toast activation (`tauri-winrt-notification`) in Phase 9; fall back to focusing the app. |
| R10 | Unsigned installer triggers SmartScreen. | Install friction. | Document; add signing step once a certificate exists. |
| R11 | Provider docs change faster than code. | Silent breakage. | Capabilities documented with versions; Diagnostics shows detected versions vs tested ranges. |
| R12 | Attributing Git changes to agents incorrectly. | Misleading UI. | Implemented: a file is linked to an agent only when its tool reported writing it; a commit only when the agent ran a commit-creating `git` command in that tree and the commit's time falls within that command's run (never when two agents qualify); everything else is shown without a name. |

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

## 7. Phase 4 checklist

- [x] `ao-config`: one safe config-file editor shared by the adapters
- [x] JSON-RPC client for `codex app-server` (responses, notifications, server requests)
- [x] Managed sessions: thread start, prompts (`turn/start`, `turn/steer`), interrupt, graceful and force stop
- [x] Approvals from the app: commands, file changes, extra permissions; timeout declines
- [x] Subagents as characters (child threads), token usage per session (no cost), errors and retries
- [x] Version check against the tested 0.156.x
- [x] External sessions through `hooks.json`: shell-quoted relay command, append-only edits, uninstall, trust status from `hooks/list`, optional permission answers
- [x] Fixtures recorded from the real CLI with a local fake model; `fake-codex` for end-to-end tests; the finished adapter checked against the real binary

### Phase 4 findings

* A Codex hook `command` is a shell line, unlike Claude's exec form: paths are
  quoted per shell and paths `cmd.exe` cannot carry safely (`"`, `%`) are refused.
* Codex keys hook trust on the hook's position, so editing a user's
  `hooks.json` must never shift existing entries (append-only, placeholders on
  removal).
* Subagent threads are only announced by the parent's `spawnAgent` item;
  there is no `thread/started` for them.
* Token usage is reported per thread; a session's usage is the sum.
* The app-server keeps retrying without network: the office shows recoverable
  errors until the user stops the session.
* Not implemented (not needed on 0.156.x): the `codex exec --json` fallback.
* Next: Cursor (Phase 5).

## 8. Phase 5 checklist

- [x] `ao-jsonrpc`: the JSON-RPC client shared by Codex and Cursor (calls without a time limit for ACP prompt turns)
- [x] ACP client for `agent acp`: initialize, `cursor_login` authentication, session/new, model (config option or older `session/set_model`), mode, prompt, cancel
- [x] Permissions answered by option kind (ids echoed), timeout → reject, cancelled answers on stop; unknown agent requests declined at once
- [x] Runtime values: context usage, USD cost, per-turn tokens, model, mode, title, compaction; `usageSnapshot.contextTokens`
- [x] `fake-cursor` + end-to-end tests; fixtures from the official ACP example agent; traffic validated against the official schema
- [x] Launch hints in *New agent* (modes agent / plan / ask, no fixed model list)
- [ ] Cursor hooks (external sessions): **not implemented** — format not verifiable here (PROVIDER_CAPABILITIES §5.5)
- [ ] Manual verification on Windows 11 with a real Cursor CLI (below)

### Phase 5 findings

* `cursor.com` (docs and CLI downloads) is blocked by the build environment's
  network policy, so the real Cursor CLI could not be run. The adapter follows the
  official ACP schema; Cursor-specific handling comes from integrator reports and
  is marked *Experimental* in the app.
* The official ACP example agent reuses tool call ids in every turn and leaves a
  rejected tool without a final update: the mapper treats a new `tool_call` with a
  finished id as a new call and closes open tools at the end of the turn.
* Cursor reportedly needs `authenticate` (`cursor_login`) before sessions work and
  offers both the ACP 1.x model config option and the older `session/set_model`.
* An ACP prompt turn is one long request; a turn is freed *before* its end is
  reported, so a new prompt sent right after "idle" is accepted.

### Manual verification on Windows 11 (needs a Cursor account)

Run with the installed Agent Office on a Windows 11 PC where the Cursor CLI is
installed and `agent login` was done once in a terminal. Note the Cursor version
(`agent --version`) next to each result.

1. **Detection** — Diagnostics → Providers shows Cursor CLI *Installed* with a
   version and the executable path (`agent.exe`, `agent.cmd`, `cursor-agent…`,
   under `%LOCALAPPDATA%\cursor-agent` or elsewhere). Record the path.
2. **Launch** — New agent → Cursor CLI, a project folder, no model, first prompt
   "List the files in this folder". The character appears, works, and says a
   message; the session shows a model name (Auto or similar).
3. **Login** — Log out (`agent logout`), launch again: the error should mention
   `agent login` (or Cursor opens its own login). Log back in afterwards.
4. **Model** — Launch with model `gpt-5` (or another model name Cursor shows in
   its own model picker). The session shows that model; an invented name shows the
   warning "Cursor did not switch to model …".
5. **Modes** — Launch with mode *plan* and with *ask*. Record which are accepted
   and whether a warning appears.
6. **Permission** — Prompt "Create a file hello.txt containing hi". A permission
   request appears on the character with Cursor's options; *Approve* → the file is
   created and a *file created/modified* event appears. Repeat with *Reject*: no
   file, the tool shows "Not allowed" or Cursor's own message.
7. **Command** — Prompt "Run `dir`" (approve): a command appears in the timeline.
8. **Stop** — During a long prompt press *Stop*: the turn ends as cancelled and
   the session ends "stopped by Agent Office"; Task Manager shows no leftover
   `agent` / `node` process from that session. *Force stop* also ends it.
9. **Extension requests** — Ask Cursor to "make a plan first" or a question that
   makes it ask you something. If a "Cursor sent `cursor/…`" warning appears, note
   the method name; the turn must continue, not hang.
10. **Usage** — Record whether usage (context tokens) and cost appear; if not, the
    panel must say "Unavailable" / "Not reported yet", never a made-up number.
11. **Logs** — Diagnostics → *Run diagnostics*, and keep
    `%LOCALAPPDATA%\AgentOffice\logs\providers.log.*` with the findings (start
    the app with `AGENT_OFFICE_LOG=info,provider=debug` for more detail; check the
    file for anything private before sharing it).

## 9. Phase 6 checklist

- [x] Project rows ("pods") with name plates; subagents sit next to their lead, overflow into the meeting room
- [x] Calm movement: a room change needs the new activity to last 1.5 s
- [x] Poses per activity (typing, reading, thinking, raised hand, worried, coffee, celebrating, walking), four hair styles, lighter shirts for subagents
- [x] Screens show the work (code, document, terminal, alert, error); thought cloud; confetti when done; fade-out at the door
- [x] Hover card, keyboard navigation (arrows, Enter, Escape), legend; name tags kept on screen
- [x] `validateLayout` for layouts; tests for layout, scene, sprites, teams, renderer and stress generator
- [x] Stress test in the preview (`/?stress`: 20 sessions + 50 subagents) and `scripts/office-bench.mjs`
- [x] 30 fps when nothing walks, 60 fps otherwise
- [ ] Confirm the frame cost on a mid-range Windows laptop (`npm run build:ui`, then the bench script)

### Phase 6 findings

* Characters used to walk to another room on every tool call; with real agents
  that alternate reading, commands and edits every second the office looked
  chaotic. The settle delay keeps them at their desk for quick commands.
* Subagents are easiest to read next to their lead; the old meeting-room
  placement made the lead → subagent lines cross the whole office. Lines are now
  drawn for the focused team only once the office holds more than 16 agents.
* Frame cost for 70 agents is about 1 ms (p95 1.5 ms) in headless Chromium in
  the build container, far under the 16.7 ms of a 60 fps frame; most time goes
  to canvas drawing, the scene logic is well under a millisecond.
* Two existing defects surfaced while running the whole test suite and were
  fixed: `ao-process` reported a killed process tree as "exited with an error"
  when the exit was noticed before the kill request (the kill intent is now
  recorded before signalling; a new test fails 4/4 with the old code), and two
  host tests could share a temporary folder when they started in the same
  millisecond.
* Not done (later phases): layout editor and saved layouts, furniture moves and
  unlockables (data types exist), provider-specific sprites.

## 10. Phase 7 checklist

- [x] Agents start suspended, are put in their own Job Object, then run: nothing they start can escape the job (killed if they cannot be resumed)
- [x] Stop (close input, wait 10 s, end the tree) and Force stop (end the tree at once)
- [x] Exit details: exit code, signal, Windows crash names, "stopped by Agent Office", processes left running by the agent (stopped and counted)
- [x] `.cmd` shims: arguments, exit codes and tree kill tested with a real shim on Windows CI
- [x] PID-reuse guard: the process handle is held for the process's whole life; nothing acts on a bare PID
- [x] Silent agents: busy but quiet for N minutes (preference, default 5) → hourglass and a note; never stopped automatically
- [x] Restart counter per session; **Restart** for Claude (`--resume`) and Codex (`thread/resume`); refused for Cursor
- [x] **Open terminal** in the session's folder (Windows Terminal, else PowerShell, else `cmd`)
- [x] Diagnostics: *Processes started by Agent Office* (PID, status, tree size, last output)
- [x] Process tests on Windows CI with fake agents (tree kill, leftovers, no foreign kill)
- [ ] On Windows 11 by hand: Open terminal with and without Windows Terminal; Restart of a real Claude and a real Codex session (uses a few tokens of the user's own account)

### Phase 7 findings

* Until this phase a new agent process was placed in its job right after it
  started; anything it launched in that first instant would have escaped. It
  now starts suspended and runs only once it is inside the job.
* When an agent exits it can leave processes behind (a dev server, a watcher).
  They belong to the finished session and would die anyway when the job is
  closed; they are now stopped at once and counted in the end reason.
* Real Codex 0.156.1 (checked locally with threads saved by earlier runs, no
  account, no cost): `thread/resume` keeps the thread id, sends no
  `thread/started`, and an unknown id fails with "no rollout found". The adapter
  announces the resumed session itself.
* Real Claude Code 2.1.282: `--resume` with an unknown id fails before any API
  call, with a clear message on stderr and an error `result` line (recorded as a
  fixture).
* Cursor: ACP's `session/load` is optional and could not be tried against a real
  Cursor, so Restart is refused for Cursor sessions instead of guessing.
* "Stuck" cannot be told apart from "running a long build" from the outside, so
  a silent agent is only flagged. The rule is per session: some agent is busy
  and no event arrived for N minutes.
* During development one real Claude prompt was sent by mistake (about
  US$0.04): a temporary `CLAUDE_CONFIG_DIR` does not hide credentials passed in
  environment variables. DEVELOPMENT.md now warns about it; tests never call a
  real provider.

## 11. Phase 8 checklist

- [x] `ao-git`: read-only Git through the user's `git.exe` (porcelain v2), parsers tested with recorded output and real temporary repositories
- [x] Repository, working tree and linked worktrees; branch, detached HEAD, upstream ahead/behind; staged / not staged / new / conflicted files; recent commits; worktree list
- [x] Out of the agents' way: no optional locks (index never rewritten), repository `core.fsmonitor` program not started, no pager/prompts/console window, timeout, no inherited `GIT_*` variables
- [x] Refresh after the agent edits files, runs commands or ends a turn (debounced), every 30 s while an agent works there, and on request
- [x] Unified events `git.branch_changed`, `git.status_changed`, `git.commit_created` and the session's repository id / worktree
- [x] Conservative attribution: files by tool evidence, commits by `git commit` evidence inside the command's run; otherwise nobody
- [x] Agent panel, Projects repository cards, Command Center column; Open project; Show changed file (selected in Explorer, never run)
- [x] Diagnostics: Git missing or older than 2.15
- [ ] On Windows 11 by hand: a project on NTFS with Git for Windows (status, a worktree, Open project, Show a file), and a Claude or Codex session committing (credited to it)

### Phase 8 findings

* A plain `git status` rewrites the index to refresh file times and can start a
  program named in the repository's `core.fsmonitor`. Both were shown with real
  repositories in the tests; Agent Office's reads do neither.
* Git reports the top folder with its long name while a session may report a
  short Windows name (`RUNNER~1`) for the same folder; file matching falls back
  to the real paths so evidence is not lost.
* "Open changed file" became "Show": opening a file with its default program
  runs `.js` (Windows Script Host), `.bat` or `.exe` files, and agents can
  create any file.
* A commit time has one-second resolution, so the evidence window allows one
  second on each side. Commits made while two agents run `git commit` in the
  same tree are credited to nobody.
* Running the whole test suite repeatedly exposed a real race in Phase 7's
  Restart: the adapter freed the session id before sending the old run's end,
  so that end could close the restarted session and count an extra restart.
  Claude and Codex now send the end first, and a resume waits for the old run
  to be released. A test that resumes right after a stop fails 3/3 with the
  old code. A Phase 7 test that checked the silence warning too early was also
  fixed.
* Not done: diffs of a file inside the app, submodules as separate repositories,
  and Git state of projects no agent works in outside the Projects view.
