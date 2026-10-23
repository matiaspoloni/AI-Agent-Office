# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

(Desktop app for Windows 11 and macOS 11+, built with Tauri: the UI is a web view — WebView2 on Windows, WKWebView on macOS — not native OS controls. Linux runs for development and CI only.)

## Users

Two audiences, served equally:

- **The developer running several agents at once.** Has Claude Code, Codex CLI and/or Cursor CLI sessions going in parallel across one or more projects and needs to see at a glance who needs them — a permission request, an error, a finished task — without hopping between terminals, then act on it.
- **The person supervising agent work.** Watches activity across several projects or repositories: who is busy, what changed, what got committed and by whom. More watching than operating.

## Product Purpose

Agent Office shows the AI coding agents you have running as employees in a living pixel-art office, and lets you act on them when the provider allows it. It turns many opaque terminal sessions into one readable picture of who is coding, testing, waiting, blocked or done, and puts the actions (approve or reject, give instructions, stop, restart, open a terminal, continue an external session) next to that picture.

Success: you notice the agent that needs you as soon as it does, understand what each one is doing without reading its log, and never lose track of a session.

## Positioning

- **Two equal views of the same truth.** The **office** is for reading state at a glance (pose, screen, room, and symbols carry the status). The **Command Center** is for operating and dense data (approvals, errors, tool calls, tokens, branches). Neither is the "real" view; both read from one unified event model.
- **Provider-agnostic.** Every provider is an adapter that turns its official events (hooks, JSON-RPC, stream-json, Agent Client Protocol) into one event model; the UI never sees provider-specific data.
- **Honest by construction.** Shows "Unavailable" rather than guessing; credits a file or commit to an agent only when a tool call proves it; labels Claude's cost as an estimate.
- **Local-first.** No accounts, no cloud, no telemetry, no auto-updater, and it never contacts a server.

## Operating Context

- Runs beside the user's terminals and editor, often in the background; OS notifications pull the user back ("Claude Code needs permission") and open the app with that agent selected.
- Two kinds of session: **managed** (launched from the app; fully controllable) and **external** (started in the user's own terminal; observed via hooks, answerable only for permission prompts if the user opts in, and continuable in the app via the provider's own resume).
- Main surfaces: Office, Command Center, Projects (per-repository cards), Diagnostics (dependency checks, hook install/repair, settings, logs, exportable report), the agent panel, the in-office instruction bubble, and the New agent dialog.
- Office interaction: hover or arrow keys for a summary, click for the panel, double-click or **I** for the instruction bubble, a **Legend** explaining every symbol.
- `npm run dev:web` runs the UI alone in a browser with a replayed demo timeline; `?stress` gives 20 sessions with 50 subagents.

## Capabilities and Constraints

- Providers: Claude Code and Codex CLI (managed and external); Cursor CLI managed only and **experimental**.
- Office layout is fixed for now (four project rows of six desks; more projects share rows). Moving furniture and unlocking objects are planned; the layout is already plain data.
- Subagents are extra employees seated next to their lead, in a lighter shirt of the same color.
- Agents are customizable per project (look, accessory, shirt, name tag) from the agent panel; every session of that project keeps the look. It is stored only in the app's local web storage, not in the core's database.
- The office zooms (scroll, pinch, +/−/0, buttons) and pans by dragging; the keyboard follows the selected agent.
- Three themes, chosen beside the zoom controls and remembered per viewer: **Modern** (default), **Cyberpunk** and **L.A. Noire**; the last two repaint both the office and the interface around it.
- Hard rules: never kills or types into a process it did not start; never opens a changed file with its default program; never changes the user's Git repositories (reads only).
- Capabilities differ per provider and per session origin; actions that do not apply stay visible but disabled, with the reason in a tooltip.
- Must stay usable at scale (tens of sessions and subagents) — there is a stress mode and an office benchmark.
- Neither package is code-signed yet. All phases are implemented and CI-tested; manual verification on a real PC and Mac with real provider accounts is still open.

## Brand Commitments

- Name: **Agent Office**. App icon at `assets/app-icon.png`, favicon at `public/favicon.png`.
- The pixel-art office metaphor — sessions as employees, rooms as activities (desks, terminal room, QA lab, meeting room, lounge, CEO office) — is the identity.
- Voice: plain, literal and honest; says what it does not know or cannot do, and why.
- Language: the interface is English today and should become **bilingual English + Spanish** (the Windows installer already follows the Windows language).

## Evidence on Hand

- Screenshot of the office with the built-in demo: `docs/images/office.png`.
- Replayable demo timeline: `src/ipc/preview-recording.json`.
- Honest provider support matrix: `docs/PROVIDER_CAPABILITIES.md`; roadmap with open manual checks: `docs/ROADMAP.md`.
- No users, testimonials, press, download counts or benchmarks against other tools exist. Do not invent them.

## Product Principles

1. **Glanceable first, actionable second.** State must be readable without opening anything; actions sit one step away from where the state is seen.
2. **Never guess.** Unknown is shown as unknown; attribution needs proof.
3. **Respect what isn't yours.** The user's processes, terminals, files and repositories are observed, never driven, unless the app started them.
4. **One model, many providers.** Nothing in the UI may depend on a single provider's quirks.
5. **Local and quiet.** No network, no accounts; notifications are few and only when the user is needed.

## Accessibility & Inclusion

- Keyboard reaches the office (arrow keys to select, **I** to instruct) and the rest of the app; the office announces changes through a polite live region for screen readers. Keep both working as the office grows.
- Status in the office must never depend on color alone: pose, screen, room and symbol already carry it.
- Text must fit both English and Spanish; Spanish strings run longer.
- No formal standard (such as WCAG AA) has been adopted yet.
