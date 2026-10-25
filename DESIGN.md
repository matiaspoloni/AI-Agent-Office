---
name: Agent Office
description: Your AI coding agents as employees in a living pixel-art office, seen through the window at night.
colors:
  desk-lamp-amber: "#f2c94c"
  night-window: "#0f1117"
  after-hours-panel: "#171a23"
  raised-panel: "#1e222d"
  window-mullion: "#2a2f3d"
  mullion-hover: "#4a5268"
  deep-well: "#0c0e13"
  fluorescent-white: "#e7e9ef"
  dusk-grey: "#8a93a6"
  ink-on-amber: "#1a1a1a"
  build-green: "#3fbf5f"
  alarm-red: "#ff5c5c"
  ember-orange: "#f2994a"
  link-blue: "#8ab4ff"
  lab-violet: "#b58cff"
  log-green: "#7bdc8b"
  log-red: "#ff8a8a"
  timestamp-slate: "#5d667a"
  unsupported-slate: "#6b7285"
  amber-notice-surface: "#2a2412"
  amber-notice-edge: "#4a3f1c"
  amber-notice-text: "#f2d98a"
  alarm-surface: "#2a1517"
  alarm-edge: "#7a3036"
  alarm-button-surface: "#472023"
  green-button-surface: "#1f3d28"
  green-button-edge: "#2f6b41"
  silence-surface: "#2a2112"
  silence-edge: "#7a5a2a"
  info-surface: "#182031"
  info-edge: "#2a3857"
  info-text: "#a9c1f5"
  cyber-night-window: "#07060f"
  cyber-panel: "#0f0d1c"
  cyber-raised-panel: "#17142a"
  cyber-mullion: "#2a2350"
  cyber-text: "#ece9ff"
  cyber-dusk: "#9a93c8"
  cyber-timestamp: "#6d64a8"
  neon-magenta: "#ff2bd6"
  neon-cyan: "#00e5ff"
  ink-on-magenta: "#12001a"
  cyber-good: "#00ff9f"
  cyber-bad: "#ff3b5c"
  cyber-warn: "#ffb800"
  cyber-good-surface: "#00301f"
  cyber-bad-surface: "#33061a"
  cyber-notice-surface: "#1f0a24"
  cyber-notice-edge: "#5a1650"
  cyber-notice-text: "#ff9be8"
  cyber-info-surface: "#071d26"
  cyber-info-edge: "#0e4a5c"
  cyber-info-text: "#7df3ff"
  cyber-alarm-surface: "#2a0614"
  cyber-silence-surface: "#241a04"
  cyber-silence-edge: "#8a6400"
  noir-night-window: "#0d0c0b"
  noir-panel: "#171512"
  noir-raised-panel: "#211e1a"
  noir-mullion: "#3a342c"
  noir-paper: "#ece4d4"
  noir-dusk: "#a39782"
  noir-brass: "#c9a24a"
  noir-ink: "#1a140c"
  noir-good: "#7fa86a"
  noir-bad: "#d0524a"
  noir-warn: "#d89a3a"
  noir-notice-surface: "#221a10"
  noir-notice-edge: "#4a3a20"
  noir-notice-text: "#e0c890"
  noir-alarm-surface: "#2a1210"
  noir-alarm-edge: "#6a2a22"
  noir-brass-edge: "#6b5530"
  noir-brass-hover: "#8a7040"
  noir-link: "#d8b86a"
  noir-timestamp: "#6e6456"
  noir-good-surface: "#1e2a18"
  noir-good-edge: "#4a6a3a"
  noir-bad-surface: "#2e1210"
  noir-bad-edge: "#7a2e26"
  noir-info-surface: "#161a1c"
  noir-info-edge: "#2e3a40"
  noir-info-text: "#b8c8cc"
  noir-silence-surface: "#24190c"
  noir-silence-edge: "#6a4a1e"
typography:
  headline:
    fontFamily: "Segoe UI, system-ui, -apple-system, sans-serif"
    fontSize: "22px"
    fontWeight: 700
  title:
    fontFamily: "Segoe UI, system-ui, -apple-system, sans-serif"
    fontSize: "14px"
    fontWeight: 700
  body:
    fontFamily: "Segoe UI, system-ui, -apple-system, sans-serif"
    fontSize: "14px"
    fontWeight: 400
  body-dense:
    fontFamily: "Segoe UI, system-ui, -apple-system, sans-serif"
    fontSize: "13px"
    fontWeight: 400
  label:
    fontFamily: "Segoe UI, system-ui, -apple-system, sans-serif"
    fontSize: "12px"
    fontWeight: 700
    letterSpacing: "0.04em"
  caption:
    fontFamily: "Segoe UI, system-ui, -apple-system, sans-serif"
    fontSize: "12px"
    fontWeight: 400
  mono:
    fontFamily: "Cascadia Code, Consolas, ui-monospace, monospace"
    fontSize: "11.5px"
    fontWeight: 400
    lineHeight: "20px"
rounded:
  pixel: "2px"
  tag: "4px"
  base: "6px"
  modal: "10px"
  pill: "999px"
spacing:
  xxs: "3px"
  xs: "4px"
  sm: "6px"
  md: "8px"
  lg: "10px"
  xl: "12px"
  xxl: "16px"
components:
  button:
    backgroundColor: "{colors.raised-panel}"
    textColor: "{colors.fluorescent-white}"
    rounded: "{rounded.base}"
    padding: "6px 12px"
  button-primary:
    backgroundColor: "{colors.desk-lamp-amber}"
    textColor: "{colors.ink-on-amber}"
    rounded: "{rounded.base}"
    padding: "6px 12px"
  button-good:
    backgroundColor: "{colors.green-button-surface}"
    textColor: "{colors.fluorescent-white}"
    rounded: "{rounded.base}"
    padding: "6px 12px"
  button-bad:
    backgroundColor: "{colors.alarm-button-surface}"
    textColor: "{colors.fluorescent-white}"
    rounded: "{rounded.base}"
    padding: "6px 12px"
  button-small:
    backgroundColor: "{colors.raised-panel}"
    textColor: "{colors.fluorescent-white}"
    typography: "{typography.caption}"
    rounded: "{rounded.base}"
    padding: "3px 8px"
  tab:
    backgroundColor: "transparent"
    textColor: "{colors.dusk-grey}"
    rounded: "{rounded.base}"
    padding: "6px 12px"
  tab-active:
    backgroundColor: "{colors.raised-panel}"
    textColor: "{colors.fluorescent-white}"
    rounded: "{rounded.base}"
    padding: "6px 12px"
  input:
    backgroundColor: "{colors.deep-well}"
    textColor: "{colors.fluorescent-white}"
    rounded: "{rounded.base}"
    padding: "6px 8px"
  card:
    backgroundColor: "{colors.after-hours-panel}"
    textColor: "{colors.fluorescent-white}"
    rounded: "{rounded.base}"
    padding: "12px 14px"
  kpi:
    backgroundColor: "{colors.after-hours-panel}"
    textColor: "{colors.fluorescent-white}"
    typography: "{typography.headline}"
    rounded: "{rounded.base}"
    padding: "10px 12px"
  chip:
    backgroundColor: "{colors.raised-panel}"
    textColor: "{colors.fluorescent-white}"
    typography: "{typography.caption}"
    rounded: "{rounded.pill}"
    padding: "1px 8px"
  status-pill:
    backgroundColor: "transparent"
    typography: "{typography.body-dense}"
    rounded: "{rounded.pill}"
    padding: "4px 10px"
  badge:
    textColor: "#101218"
    rounded: "{rounded.tag}"
    padding: "1px 5px"
  notice:
    backgroundColor: "{colors.amber-notice-surface}"
    textColor: "{colors.amber-notice-text}"
    typography: "{typography.caption}"
    rounded: "{rounded.base}"
    padding: "6px 10px"
  notice-subtle:
    backgroundColor: "{colors.info-surface}"
    textColor: "{colors.info-text}"
    rounded: "{rounded.base}"
    padding: "6px 10px"
  permission-box:
    backgroundColor: "{colors.alarm-surface}"
    rounded: "{rounded.base}"
    padding: "10px"
  log:
    backgroundColor: "{colors.deep-well}"
    typography: "{typography.mono}"
    rounded: "{rounded.base}"
    padding: "0 6px"
  modal:
    backgroundColor: "{colors.after-hours-panel}"
    rounded: "{rounded.modal}"
    padding: "16px 18px"
    width: "min(560px, 92vw)"
  agent-panel:
    backgroundColor: "{colors.after-hours-panel}"
    padding: "12px"
    width: "380px"
---

# Design System: Agent Office

## Overview

**Creative North Star: "The Night Office"**

You are standing outside an office building at night, looking in through the window. Inside, under desk lamps, the employees are working: typing, reading, raising a hand, slumping after an error. That lit pixel-art room is the only warm, colorful, animated thing on screen. Everything around it (the top bar, the agent panel, the Command Center, Diagnostics) is the dark glass and window frame: low-contrast navy-charcoal surfaces, thin borders, quiet grey type, there to frame the room and hand you the controls, never to compete with it.

The interface is dense and calm, built for someone who glances at it between other work. Color is spent almost entirely on meaning: the amber desk-lamp accent marks the one action that matters and the focused element; green, red, orange and violet mark state. Surfaces are flat and separated by one-pixel borders and tonal steps, not shadows. The pixel world lends the chrome a few deliberate echoes (hard pixel corners on legend glyphs, pixelated icons, a crisp border on the floating instruction bubble), placed sparingly so the frame still reads as frame.

This is not a SaaS metrics dashboard: no gradients, no glass blur, no decorative charts, no hero numbers for their own sake. A number earns large type only when it is a count the user acts on.

**Key Characteristics:**
- One dark, flat, tonal chrome; one warm, lit pixel world inside it.
- Amber is the single accent; every other hue is a state.
- Dense 12–14px system type; monospace only for logs, paths and code.
- Depth from tonal steps and 1px borders; shadows only for things that float over the office.
- Pixel echoes in the chrome are punctual, never a theme applied everywhere.

## Colors

A cool navy-charcoal night palette with one warm desk-lamp amber, and a small set of saturated state colors that each mean exactly one thing.

### Primary
- **Desk-Lamp Amber** (`desk-lamp-amber`): the primary button, the keyboard focus ring on the office canvas, the border of the floating instruction bubble, and warning-level log lines. It is the light in the window. Text on it is always **Ink on Amber**, never white.

### Neutral
- **Night Window** (`night-window`): the app background behind everything.
- **After-Hours Panel** (`after-hours-panel`): the top bar, agent panel, cards, KPI tiles and modals.
- **Raised Panel** (`raised-panel`): buttons at rest, the active tab, chips, table-row hover, toasts.
- **Deep Well** (`deep-well`): sunken surfaces that hold user or machine text: inputs, selects, textareas and the log.
- **Window Mullion** (`window-mullion`): every 1px border and divider; **Mullion Hover** (`mullion-hover`) is the only hover change on a default button.
- **Fluorescent White** (`fluorescent-white`): primary text. **Dusk Grey** (`dusk-grey`): secondary text, labels, inactive tabs, table headers, the `kv` labels. **Timestamp Slate** and **Unsupported Slate** sit one step quieter still, for log times and unsupported capabilities.

### State colors
- **Build Green** (`build-green`): healthy, done, supported, a passing test. **Log Green** is its lighter text form on Deep Well.
- **Alarm Red** (`alarm-red`): errors and failed checks. **Log Red** is its text form on dark surfaces.
- **Ember Orange** (`ember-orange`): warnings, "no news for N minutes", git change codes.
- **Lab Violet** (`lab-violet`): experimental capabilities (Cursor).
- **Link Blue** (`link-blue`): inline text links only.

### Tinted surfaces
Each state that needs a box gets a dark tinted surface plus a mid-tone edge, never a saturated fill: **Amber Notice** (banner and notices: surface, edge and pale-amber text), **Alarm** (permission box and error toasts), **Silence** (the stale-agent box), **Info** (subtle notices), and the **Green / Alarm button surfaces** behind Approve and Reject.

### The office palette
The pixel world draws its own colors on the canvas and is deliberately warmer and lighter than the chrome. It is a present-day tech office: polished concrete in the open space and corridors (`#676c78`, `#7d818b`, aggregate specks and a slab seam every four tiles), light grey resin in the QA lab, walnut boards in the CEO office, light oak in the lounge, charcoal carpet tiles laid in alternating directions in the meeting room, and raised-floor vent tiles in the terminal room, all inside a void of `#151821`. Walls between spaces are thin glass partitions on aluminium rails (`#aeb6c4` / `#6f788b`, a pale blue pane) that show the floor through them; only the building's edge is solid, with floor-to-ceiling glazing onto a night skyline. Furniture is white and graphite on thin dark legs: white desks with two monitors on stands, ergonomic mesh chairs, a long white meeting table with laptops, glass boards with sticky notes, an espresso machine, a sage modular sofa, big plants in white pots, black racks with blue LEDs, and a pink neon `</>` on the lounge glass. Characters vary in hair, skin and shirt; screens glow for what their user is doing. Status icons use their own pixel palette (`#ff4d4d`, `#f08a24`, `#3fbf5f`, `#4c8df5` outlined in `#2b2f3a`). Keep chrome tokens out of the canvas and canvas colors out of the chrome.

The office is lit for night. Floors sit under a navy dimming (`rgba(6,8,22,0.48)` at its darkest) broken by pools of warm lamplight over the furniture people use and along the corridors, quantized to 1px blocks in four steps. Each lamp lights only its own room; outside the building stays dark. Active screens spill a soft glow of their screen color (blue for code, white for reading, green for commands and tests, red for permission and errors, orange for waiting) onto the desk. The outer wall carries windows onto a night sky with a star and a few city lights. Characters are HD sprites: the 12×16 pose grids are doubled with Scale2x (EPX), lit from the top left (highlight on each material's top edge, shade on its bottom and right edges, a shine in the eyes) and wear a one-art-pixel `#14161e` outline, and cast a small foot shadow when standing; furniture casts a short contact shadow; walls cast a 4px shadow onto the floor below.

Characters can be dressed per project from the agent panel's **Appearance** section: hair style (short, long, bun, cap), hair color, skin tone, shirt color, one accessory (glasses, headphones, beard, bow) and a name tag for the project's main agents. Unset choices fall back to the look derived from the agent key. The shirt defaults to the provider's accent; a custom shirt is allowed because the provider badge on the name tag still identifies the provider. Subagents always wear a lighter shade of their lead's shirt. Accessories are pixel patches on the head drawn before the pose, so raised or worried hands go over them.

**The Lamplight Rule.** Light in the office comes from where people work. A lamp lights its own room only, light is stepped (never a smooth gradient), and characters are never dimmed: the night falls on the floor, not on the people.

### Named Rules
**The Desk-Lamp Rule.** Amber marks the one primary action in view and the focused element. If two amber buttons appear in the same panel, one of them is wrong.

**The One Meaning Rule.** A state color means one thing everywhere: green is healthy or done, red is error or needs-permission, orange is warning or silence, violet is experimental. Never use them for decoration or category coding.

**The Tinted Box Rule.** State boxes use the dark tinted surface plus its edge color, never a saturated fill with white text.

### Cyberpunk theme
A second, user-selectable theme (Modern / Cyberpunk, beside the office zoom controls; remembered per viewer). It sets `data-theme="cyberpunk"` on `<html>`, which redefines the chrome tokens: Night Window `#07060f`, After-Hours Panel `#0f0d1c`, Raised Panel `#17142a`, Window Mullion `#2a2350`, text `#ece9ff`, Dusk Grey `#9a93c8`, and the accent becomes **Neon Magenta** `#ff2bd6` with **Neon Cyan** `#00e5ff` as its partner (links, focus on fields, section labels, hovered buttons, the hover card's edge). States become `#00ff9f` (good), `#ff3b5c` (bad) and `#ffb800` (warn); tinted boxes shift to magenta, cyan and red-violet surfaces. In the office the same layout is painted as a neon city at night: metal grating, wet asphalt corridors with neon puddles, black marble, a purple lounge carpet, holographic glass partitions in magenta and cyan, desks and chairs in black with neon trim and RGB keys, a hologram on the meeting table, holo boards, a vending machine, bioluminescent plants, a skyline in the glazing with rain running down it, and lamps that tint each room magenta, cyan or violet.

**The Neon Pair Rule.** In the cyberpunk theme, magenta is the action and cyan is the guide: magenta for the one primary action, selection and borders that frame; cyan for focus, links and labels. A third neon only appears inside the office canvas.

### L.A. Noire theme
The third theme: a Los Angeles police bureau in 1947, entirely in black and white: the whole app is rendered through a grayscale filter, so the office, provider badges, status dots and swatches all lose their color; states stay readable through pose, icon and room in the office and through text in the interface. `data-theme="noire"` turns the chrome into aged paper `#ece4d4` on dark wood browns (`#0d0c0b`, `#171512`, `#211e1a`) with **Brass** `#c9a24a` as the accent and dark ink `#1a140c` on it; headings and section labels go uppercase and letter-spaced, Art Deco style. There is no glow: noir is shadow, not light. In the office: tungsten lamplight in one warm tint, rain on the glazing, **venetian-blind light** falling in slanted slats below every outer window, and a film vignette over the view. Linoleum in the bullpen, terrazzo halls, bare concrete in the interrogation room (a steel table under one bulb, a case file, an ashtray), white lab tile, oak boards, a burgundy rug; frosted glass partitions in wooden frames; oak desks with ivory typewriter keys; filing cabinets where the racks stand; evidence cork boards with photos and red string; a mahogany captain's desk with a green inlay, banker's lamp and rotary phone; a leather chesterfield, a percolator, potted palms, law books, brass name plates and a red-orange neon.

**The Shadow Rule.** In L.A. Noire, depth and mood come from darkness, slats of light and the vignette; never from glow or gradients on chrome. Everything is black and white, so nothing in this theme may rely on hue: a state needs its shape, icon or words.

## Typography

**Body Font:** Segoe UI (with system-ui, -apple-system, sans-serif)
**Mono Font:** Cascadia Code (with Consolas, ui-monospace, monospace)

**Character:** the operating system's own UI face, so the chrome feels native and recedes on both Windows and macOS; Cascadia Code marks anything a machine wrote: logs, file paths, commands.

### Hierarchy
- **Headline** (700, 22px): KPI values in the Command Center only.
- **Title** (700, 14px): card and modal headings.
- **Body** (400, 14px): the default for the whole app.
- **Body Dense** (400, 13px): tables, key-value rows, form labels, status pills.
- **Label** (700, 12px, 0.04em, uppercase): agent-panel section headings, always in Dusk Grey.
- **Caption** (400, 12px): header stats, small buttons, chips, notices, KPI labels.
- **Mono** (400, 11.5px, 20px line height): log rows; 12px for file lists.

### Named Rules
**The Machine Voice Rule.** Monospace is for text a machine produced or will consume (log lines, paths, commands, git codes). Human-facing copy is never monospace.

**The Small Scale Rule.** The chrome lives between 11.5px and 14px. Only KPI counts go larger. Spanish strings run 20–30% longer than English, so labels wrap or truncate with an ellipsis rather than relying on fixed widths.

## Layout

A full-height column: a 1px-bordered top bar (brand, tabs, header stats pushed right, actions), an optional banner, then the main area. The main area is a single column; selecting an agent turns it into a two-column grid with a fixed 380px agent panel on the right, divided by a 1px left border.

The office view is a full-bleed canvas filling its area, with floating overlays positioned over it: the hover card, the legend (top right, max 340px), the instruction bubble (max 320px) and the empty-state note (bottom center). The other views (Command Center, Projects, Diagnostics) are scrolling pages with 16px padding, built from cards (12px bottom margin), an auto-fill KPI grid (min 150px columns, 10px gap) and plain two- or three-column grids (12px gap).

Spacing runs on a tight 3–16px scale; 8px is the default gap between controls, 6px inside compact groups, 10–12px between panel sections and cards. Key-value rows use a fixed 110px label column.

The canvas is laid out on a 16px tile grid but drawn at HD density: two art pixels per layout pixel (`ART = 2`), on an offscreen world canvas that is then scaled to the screen as one image in whole or half steps (minimum 1×) with nearest-neighbor rendering and centered offsets, so pixels never blur. Details may be half a layout pixel (one art pixel) thick: floor specks and seams, glass rails, key rows, code lines on screens.

### Named Rules
**The Whole Pixel Rule.** The office is only ever scaled in integer or half-integer steps with `image-rendering: pixelated`, and everything in it sits on the art-pixel grid (multiples of half a layout pixel). Never let the browser smooth it.

## Elevation & Depth

The chrome is flat. Depth comes from tonal steps (Night Window → After-Hours Panel → Raised Panel, with Deep Well sunk below) and 1px Window Mullion borders. Shadows exist only on elements that float over the office canvas, where they separate a layer from a moving scene; translucent panel backgrounds (96–98% opacity) on those overlays let the room show faintly behind them.

### Shadow Vocabulary
- **Hover lift** (`box-shadow: 0 6px 18px rgba(0, 0, 0, 0.35)`): the character hover card.
- **Bubble lift** (`box-shadow: 0 8px 22px rgba(0, 0, 0, 0.45)`): the instruction bubble, which also carries an amber border because it is where you type.
- **Modal scrim** (`rgba(0, 0, 0, 0.55)` backdrop): behind dialogs; the modal itself has no shadow.

### Named Rules
**The Flat Chrome Rule.** Cards, panels, tables and buttons never get shadows. A shadow means "this floats over the live office". The one exception is the cyberpunk theme's neon glow (a zero-offset blur in magenta or cyan) on the top bar, the agent panel's edge, the active tab, the primary button and hovered or pressed controls: it is light, not depth, and never appears in the modern theme.

## Shapes

Gently softened rectangles: a single 6px radius on almost everything (buttons, tabs, cards, inputs, notices, toasts, overlays), 10px on modals, 4px on count badges, full pills for chips and status pills, and a hard 2px on pixel legend glyphs. Borders are always 1px solid. The pixel echoes in the chrome come from hard corners and pixelated rendering on glyphs and icons, not from rounding.

### Named Rules
**The Pixel Echo Rule.** Pixel-art treatment (hard 2px corners, `image-rendering: pixelated`, stepped shapes) is reserved for glyphs, icons and elements that belong to or point at the office: the brand mark, legend symbols, the instruction bubble. Do not pixelate ordinary buttons, cards or tables.

## Components

### Buttons
Quiet, compact, tactile through their border rather than their fill.
- **Shape:** gently softened (6px), 1px border.
- **Default:** Raised Panel fill, Window Mullion border, Fluorescent White text, 6px 12px padding.
- **Primary:** Desk-Lamp Amber fill and border, Ink on Amber text, weight 600. One per view (New agent, Send, Continue).
- **Good / Bad:** dark green or dark red tinted fills with mid-tone borders, for Approve and Reject.
- **Small:** 3px 8px, 12px text, for in-row actions.
- **Hover:** only the border brightens to Mullion Hover. **Disabled:** 45% opacity, not-allowed cursor; disabled actions stay visible with the reason in a tooltip.
- **Icon and link buttons:** no chrome at all; Dusk Grey icons, Link Blue text links.

### Tabs
Top-bar navigation between Office, Command Center, Projects and Diagnostics. Inactive tabs are bare Dusk Grey text; hover lifts the text to Fluorescent White; the active tab gains a Raised Panel fill and a Window Mullion border, like a lit window.

### Chips and pills
- **Chip:** full pill, Raised Panel fill, 1px border, 12px text, for branches and tags.
- **Status pill:** full pill, transparent fill, 1px border and text in the state's color, 13px, at the top of the agent panel.
- **Badge:** 4px radius, a state-colored fill with near-black text, 11px bold, minimum 26px wide, for counts.

### Cards / Containers
- **Corner Style:** 6px.
- **Background:** After-Hours Panel on Night Window.
- **Shadow Strategy:** none (Flat Chrome Rule).
- **Border:** 1px Window Mullion.
- **Internal Padding:** 12px 14px for cards, 10px 12px for KPI tiles.

### Inputs / Fields
- **Style:** Deep Well fill, 1px Window Mullion border, 6px radius, 6px 8px padding; labels sit above in 13px Dusk Grey.
- **Focus:** the platform focus ring; the office canvas uses a 2px Desk-Lamp Amber outline inset by 2px.
- **Checkboxes:** inline label in Fluorescent White, box aligned to the first line.

### Notices, state boxes and toasts
- **Notice:** Amber Notice surface, edge and pale-amber text at 12px; the subtle variant uses the Info surface and blue text.
- **Permission box:** Alarm surface with Alarm edge; holds the request and the Approve / Reject pair.
- **Silence box:** Silence surface with its ember edge, for an agent with no news.
- **Toasts:** bottom right, Raised Panel, max 420px, clickable; error toasts take the Alarm surface.

### Agent panel
The 380px right column: header with title and status pill, then sections divided by 1px top borders with uppercase Dusk Grey labels, key-value rows (110px label column), file lists in mono, the log, and a prompt box at the bottom.

### Log
Deep Well surface, 1px border, Cascadia Code at 11.5px on 20px rows, one line per entry with ellipsis; timestamps in Timestamp Slate; lines tinted Log Green, Log Red, amber or Dusk Grey by level.

### The Office (signature component)
A pixelated canvas on a 16px tile grid. Each agent is a character whose pose, screen, room and pixel icon carry its state, so status never depends on color alone. Floating over it: the **hover card** (translucent After-Hours Panel, hover-lift shadow, max 280px, pointer-transparent), the **Legend** (top right, pixel glyphs on white 2px squares), the **instruction bubble** (amber-bordered, bubble-lift shadow, opened by double-click or **I**), and the **empty-office note** (centered at the bottom). The canvas is keyboard-focusable and announces changes through a polite live region.

Life in the room is a hint, never noise: an agent idle for 3 minutes drops the coffee and dozes under a **zzz** bubble; one waiting on you for 2 minutes waves twice as fast; a subagent that finishes within reach of its lead high-fives it (a small spark between them). A grey tabby cat (`src/office/pet.ts`, drawn in HD like the characters) wanders the floors between naps and, when anyone is stuck (permission, input or error), curls up asleep beside whoever has been stuck the longest: one more pointer to where to look. In the Modern theme the light follows the viewer's local hour (`dayPhase`): dawn 6–7, day 7–17 (a blue sky, the dimming almost gone, city lights off), dusk 17–20 (violet to orange), night otherwise; Cyberpunk and L.A. Noire stay at night.

## Do's and Don'ts

### Do:
- **Do** keep the chrome on the Night Window / After-Hours Panel / Raised Panel / Deep Well steps with 1px Window Mullion borders.
- **Do** use Desk-Lamp Amber for exactly one primary action per view and for focus on the office canvas.
- **Do** give every state box a dark tinted surface and a mid-tone edge (the Tinted Box Rule).
- **Do** carry state through shape, icon or text as well as color, in the chrome as the office already does.
- **Do** keep pixel echoes punctual: glyphs, icons, the brand mark and things that float over the office.
- **Do** scale the office only in whole or half steps with pixelated rendering.
- **Do** size labels and buttons for Spanish as well as English.

### Don't:
- **Don't** make it look like a generic SaaS dashboard: no gradients, no glassmorphism or backdrop blur, no decorative charts, no oversized hero metrics, no gradient-filled cards.
- **Don't** add shadows to cards, panels, tables or buttons; shadows are only for overlays on the office.
- **Don't** use a state color for decoration or category coding.
- **Don't** put white text on amber, or saturated fills behind long text.
- **Don't** use monospace for human-facing copy.
- **Don't** pixelate ordinary buttons, cards or tables, or bring chrome tokens into the canvas.
