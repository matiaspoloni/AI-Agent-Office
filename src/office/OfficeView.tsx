import { useEffect, useMemo, useRef, useState } from "react";
import type { Project } from "../bindings/Project";
import type { SessionState } from "../bindings/SessionState";
import { ACTIVITY_COLOR, ACTIVITY_LABEL, formatDuration } from "../state/format";
import { useOfficeStore } from "../state/store";
import { DEFAULT_LAYOUT } from "./layout";
import { type Hitbox, OfficeRenderer } from "./renderer";
import { OfficeScene, type TeamResolver } from "./scene";
import { ICON_PALETTE, ICONS, spriteCanvas } from "./sprites";
import { teamResolver } from "./teams";

/** Frame pacing: full rate while someone walks, half rate when the office is calm. */
const BUSY_FRAME_MS = 1000 / 60;
const CALM_FRAME_MS = 1000 / 30;

/**
 * Hooks for scripts/office-bench.mjs: when the page defines
 * `window.__officeBench = { frames: [] }` before loading, every frame is
 * drawn at full rate and its cost (sync + step + draw, in ms) is recorded.
 */
interface OfficeBench {
  frames: number[];
}
function bench(): OfficeBench | undefined {
  return (window as unknown as { __officeBench?: OfficeBench }).__officeBench;
}

/** Agents in reading order (row by row, left to right) for keyboard navigation. */
function readingOrder(scene: OfficeScene): string[] {
  return [...scene.entities.values()]
    .filter((e) => e.fadeMs === 0)
    .sort((a, b) => Math.round(a.y / 16) - Math.round(b.y / 16) || a.x - b.x)
    .map((e) => e.key);
}

/**
 * The office canvas. Runs its own requestAnimationFrame loop and reads the
 * store directly (getState), so incoming events never re-render React here.
 */
export function OfficeView() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const cardRef = useRef<HTMLDivElement>(null);
  const hoverRef = useRef<string | null>(null);
  const hitboxesRef = useRef<Hitbox[]>([]);
  const sceneRef = useRef<OfficeScene | null>(null);
  const [hoverKey, setHoverKey] = useState<string | null>(null);
  const [keyboardKey, setKeyboardKey] = useState<string | null>(null);
  const [legend, setLegend] = useState(false);
  const [announcement, setAnnouncement] = useState("");
  const [cursor, setCursor] = useState("default");

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const scene = new OfficeScene(DEFAULT_LAYOUT);
    sceneRef.current = scene;
    const renderer = new OfficeRenderer(DEFAULT_LAYOUT);
    let frame = 0;
    let last = performance.now();
    let lastDraw = 0;
    let moving = true;
    let cssWidth = 0;
    let cssHeight = 0;
    let teams: { sessions: Record<string, SessionState>; projects: Project[]; resolve: TeamResolver } | null = null;

    const resize = () => {
      const rect = canvas.getBoundingClientRect();
      const dpr = window.devicePixelRatio || 1;
      cssWidth = rect.width;
      cssHeight = rect.height;
      canvas.width = Math.max(1, Math.round(rect.width * dpr));
      canvas.height = Math.max(1, Math.round(rect.height * dpr));
      lastDraw = 0;
    };
    const observer = new ResizeObserver(resize);
    observer.observe(canvas);
    resize();

    const probe = bench();
    const loop = (t: number) => {
      frame = requestAnimationFrame(loop);
      if (t - lastDraw < (moving || probe ? BUSY_FRAME_MS : CALM_FRAME_MS) - 1) return;
      const started = performance.now();
      const dt = Math.min(100, t - last);
      last = t;
      lastDraw = t;
      const state = useOfficeStore.getState();
      const now = Date.now();
      if (!teams || teams.sessions !== state.sessions || teams.projects !== state.projects) {
        teams = { sessions: state.sessions, projects: state.projects, resolve: teamResolver(state.sessions, state.projects) };
      }
      scene.sync(Object.values(state.agents), now, teams.resolve);
      moving = scene.step(dt);
      let pendingApprovals = 0;
      for (const a of Object.values(state.agents)) if (a.pendingPermission && !a.ended) pendingApprovals++;
      const dpr = window.devicePixelRatio || 1;
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      // Draw in device pixels; hitboxes are converted back to CSS pixels.
      const boxes = renderer.draw(ctx, cssWidth * dpr, cssHeight * dpr, {
        scene,
        agents: state.agents,
        providers: state.providers,
        selectedKey: state.selectedAgentKey,
        hoverKey: hoverRef.current,
        pendingApprovals,
        now,
      });
      hitboxesRef.current = boxes.map((b) => ({ ...b, x: b.x / dpr, y: b.y / dpr, w: b.w / dpr, h: b.h / dpr }));
      probe?.frames.push(performance.now() - started);
      // Keep the hover card next to its character while it walks.
      const card = cardRef.current;
      const hovered = hoverRef.current ? boxes.find((b) => b.key === hoverRef.current) : undefined;
      if (card && hovered) {
        const x = hovered.x / dpr + hovered.w / dpr + 8;
        const y = hovered.y / dpr - 4;
        card.style.left = `${Math.min(x, cssWidth - card.offsetWidth - 8)}px`;
        card.style.top = `${Math.max(8, Math.min(y, cssHeight - card.offsetHeight - 8))}px`;
      }
    };
    frame = requestAnimationFrame(loop);

    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
      sceneRef.current = null;
    };
  }, []);

  const setHover = (key: string | null) => {
    hoverRef.current = key;
    setHoverKey(key);
  };

  const hitAt = (event: React.MouseEvent<HTMLCanvasElement>): Hitbox | undefined => {
    const rect = event.currentTarget.getBoundingClientRect();
    const x = event.clientX - rect.left;
    const y = event.clientY - rect.top;
    // Last drawn = top-most; search from the end.
    const boxes = hitboxesRef.current;
    for (let i = boxes.length - 1; i >= 0; i--) {
      const b = boxes[i];
      if (x >= b.x && x <= b.x + b.w && y >= b.y && y <= b.y + b.h) return b;
    }
    return undefined;
  };

  const onKeyDown = (event: React.KeyboardEvent<HTMLCanvasElement>) => {
    const scene = sceneRef.current;
    const store = useOfficeStore.getState();
    if (!scene) return;
    if (event.key === "Escape") {
      store.select(null);
      setKeyboardKey(null);
      setHover(null);
      return;
    }
    if (event.key === "Enter" || event.key === " ") {
      if (keyboardKey) store.select(keyboardKey);
      event.preventDefault();
      return;
    }
    const step = event.key === "ArrowRight" || event.key === "ArrowDown" ? 1 : event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 0;
    if (!step) return;
    event.preventDefault();
    const order = readingOrder(scene);
    if (!order.length) return;
    const current = keyboardKey ?? store.selectedAgentKey;
    const index = current ? order.indexOf(current) : -1;
    const next = order[(index + step + order.length) % order.length] ?? order[0];
    setKeyboardKey(next);
    setHover(next);
    const agent = store.agents[next];
    if (agent) {
      const provider = store.providers[agent.provider]?.descriptor.displayName ?? agent.provider;
      setAnnouncement(`${agent.name}, ${provider}, ${ACTIVITY_LABEL[agent.activity]}. Press Enter for details.`);
    }
  };

  return (
    <div className="office-canvas-wrap">
      <canvas
        ref={canvasRef}
        className="office-canvas"
        style={{ cursor }}
        tabIndex={0}
        aria-label="Office view: each character is an agent session. Use the arrow keys to move between agents and Enter to open one."
        onKeyDown={onKeyDown}
        onBlur={() => {
          setKeyboardKey(null);
          if (keyboardKey) setHover(null);
        }}
        onMouseMove={(e) => {
          const hit = hitAt(e);
          const key = hit?.kind === "agent" ? hit.key : null;
          if (key !== hoverRef.current) setHover(key);
          setCursor(hit ? "pointer" : "default");
        }}
        onMouseLeave={() => {
          setHover(null);
          setCursor("default");
        }}
        onClick={(e) => {
          const hit = hitAt(e);
          const store = useOfficeStore.getState();
          if (hit?.kind === "inbox") store.setView("command");
          else store.select(hit?.kind === "agent" ? hit.key : null);
        }}
      />
      {hoverKey && <HoverCard ref={cardRef} agentKey={hoverKey} />}
      <div className="sr-only" aria-live="polite">
        {announcement}
      </div>
      <button
        className="btn small office-legend-toggle"
        aria-expanded={legend}
        onClick={() => setLegend((v) => !v)}
        title="What the symbols mean"
      >
        {legend ? "Close legend" : "Legend"}
      </button>
      {legend && <Legend />}
    </div>
  );
}

function HoverCard({ agentKey, ref }: { agentKey: string; ref: React.Ref<HTMLDivElement> }) {
  const agent = useOfficeStore((s) => s.agents[agentKey]);
  const session = useOfficeStore((s) => (agent ? s.sessions[agent.sessionKey] : undefined));
  const provider = useOfficeStore((s) => (agent ? s.providers[agent.provider]?.descriptor : undefined));
  const parent = useOfficeStore((s) => (agent?.parentKey ? s.agents[agent.parentKey] : undefined));
  const projects = useOfficeStore((s) => s.projects);
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, []);
  const team = useMemo(
    () => (agent && session ? teamResolver({ [session.key]: session }, projects)(agent).name : undefined),
    [agent, session, projects],
  );
  if (!agent) return null;
  return (
    <div className="office-hovercard" ref={ref} role="tooltip">
      <div className="office-hovercard-title">
        <span className="badge" style={{ background: provider?.accentColor }}>
          {provider?.badge ?? "?"}
        </span>
        <strong>{agent.name}</strong>
      </div>
      <div className="muted small">
        {provider?.displayName ?? agent.provider}
        {parent ? ` · subagent of ${parent.name}` : agent.isMain ? "" : " · subagent"}
        {agent.agentType ? ` · ${agent.agentType}` : ""}
      </div>
      {team && <div className="small">Project: {team}</div>}
      <div className="small">
        <span className="dot" style={{ background: ACTIVITY_COLOR[agent.activity] }} /> {ACTIVITY_LABEL[agent.activity]}
        <span className="muted"> · {formatDuration(now - agent.activitySince)}</span>
      </div>
      {agent.currentAction && <div className="small office-hovercard-action">{agent.currentAction}</div>}
      <div className="muted small">Click for details</div>
    </div>
  );
}

const LEGEND: { icon: string; text: string }[] = [
  { icon: "alert", text: "Waiting for your permission (also on the CEO desk)" },
  { icon: "question", text: "Waiting for your answer" },
  { icon: "error", text: "Something went wrong" },
  { icon: "code", text: "Writing code (at the desk)" },
  { icon: "doc", text: "Reading or searching" },
  { icon: "terminal", text: "Running a command (terminal room)" },
  { icon: "flask", text: "Running tests (QA lab)" },
  { icon: "coffee", text: "Idle, waiting for a new prompt (lounge)" },
  { icon: "check", text: "Finished: celebrates, then leaves" },
];

function Legend() {
  const images = useMemo(
    () =>
      Object.fromEntries(
        LEGEND.map(({ icon }) => [icon, spriteCanvas(ICONS[icon], ICON_PALETTE, `icon:${icon}`)?.toDataURL() ?? ""]),
      ),
    [],
  );
  return (
    <div className="office-legend" role="dialog" aria-label="Office legend">
      <p className="small">
        Each person is an agent session; the shirt color and badge show the provider. Each project gets its own row
        of desks, and subagents sit next to their lead (a dotted line links them).
      </p>
      <ul>
        {LEGEND.map(({ icon, text }) => (
          <li key={icon} className="small">
            <img src={images[icon]} alt="" width={14} height={14} className="pixel" /> {text}
          </li>
        ))}
        <li className="small">
          <span className="legend-cloud">…</span> Thinking
        </li>
      </ul>
    </div>
  );
}
