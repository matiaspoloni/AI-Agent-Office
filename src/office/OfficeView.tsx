import { useEffect, useRef, useState } from "react";
import { useOfficeStore } from "../state/store";
import { DEFAULT_LAYOUT } from "./layout";
import { type Hitbox, OfficeRenderer } from "./renderer";
import { OfficeScene } from "./scene";

/**
 * The office canvas. Runs its own requestAnimationFrame loop and reads the
 * store directly (getState), so incoming events never re-render React here.
 */
export function OfficeView() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const hoverRef = useRef<string | null>(null);
  const hitboxesRef = useRef<Hitbox[]>([]);
  const [cursor, setCursor] = useState("default");

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const scene = new OfficeScene(DEFAULT_LAYOUT);
    const renderer = new OfficeRenderer(DEFAULT_LAYOUT);
    let frame = 0;
    let last = performance.now();
    let cssWidth = 0;
    let cssHeight = 0;

    const resize = () => {
      const rect = canvas.getBoundingClientRect();
      const dpr = window.devicePixelRatio || 1;
      cssWidth = rect.width;
      cssHeight = rect.height;
      canvas.width = Math.max(1, Math.round(rect.width * dpr));
      canvas.height = Math.max(1, Math.round(rect.height * dpr));
    };
    const observer = new ResizeObserver(resize);
    observer.observe(canvas);
    resize();

    const loop = (t: number) => {
      const dt = Math.min(100, t - last);
      last = t;
      const state = useOfficeStore.getState();
      const now = Date.now();
      scene.sync(Object.values(state.agents), now);
      scene.step(dt);
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
      frame = requestAnimationFrame(loop);
    };
    frame = requestAnimationFrame(loop);

    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
    };
  }, []);

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

  return (
    <div className="office-canvas-wrap">
      <canvas
        ref={canvasRef}
        className="office-canvas"
        style={{ cursor }}
        aria-label="Office view: each character is an agent session. Click a character for details."
        onMouseMove={(e) => {
          const hit = hitAt(e);
          hoverRef.current = hit?.kind === "agent" ? hit.key : null;
          setCursor(hit ? "pointer" : "default");
        }}
        onMouseLeave={() => {
          hoverRef.current = null;
          setCursor("default");
        }}
        onClick={(e) => {
          const hit = hitAt(e);
          const store = useOfficeStore.getState();
          if (hit?.kind === "inbox") store.setView("command");
          else store.select(hit?.kind === "agent" ? hit.key : null);
        }}
      />
    </div>
  );
}
