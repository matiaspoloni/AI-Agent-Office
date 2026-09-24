// Office performance benchmark: opens the browser preview's stress test
// (`/?stress`: 20 simulated sessions + 50 subagents), lets everyone walk in,
// then records how long each frame takes to update and draw.
//
//   npm run build:ui && node scripts/office-bench.mjs [--seconds 10] [--width 1600 --height 1000] [--dpr 2]
//
// Needs Playwright with a Chromium build (not a project dependency):
// `npm i -g playwright && npx playwright install chromium`, or point
// PLAYWRIGHT_MODULE at an existing install (e.g. .../node_modules/playwright/index.mjs).
// A frame fits a 60 fps display when it takes well under 16.7 ms.
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { pathToFileURL } from "node:url";

const arg = (name, fallback) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 ? Number(process.argv[i + 1]) : fallback;
};
const seconds = arg("seconds", 10);
const width = arg("width", 1600);
const height = arg("height", 1000);
const port = arg("port", 4179);
const dpr = arg("dpr", 1);

async function loadPlaywright() {
  const candidates = [process.env.PLAYWRIGHT_MODULE, "playwright"].filter(Boolean);
  for (const name of candidates) {
    try {
      return await import(existsSync(name) ? pathToFileURL(name).href : name);
    } catch {
      // try the next one
    }
  }
  console.error("Playwright is not installed. See the header of scripts/office-bench.mjs.");
  process.exit(2);
}

if (!existsSync("dist/index.html")) {
  console.error("dist/ is missing: run `npm run build:ui` first.");
  process.exit(2);
}

const { chromium } = await loadPlaywright();
// Start Vite's own entry with this Node (no npx/shell wrapper), so killing
// the child really stops the server.
const server = spawn(process.execPath, ["node_modules/vite/bin/vite.js", "preview", "--port", String(port), "--strictPort"], {
  stdio: "ignore",
});
const stop = () => server.kill();
process.on("exit", stop);

let browser;
try {
  // Wait for the preview server.
  for (let i = 0; i < 50; i++) {
    try {
      await fetch(`http://localhost:${port}/`);
      break;
    } catch {
      await new Promise((r) => setTimeout(r, 200));
    }
  }
  browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width, height }, deviceScaleFactor: dpr });
  await page.addInitScript(() => {
    window.__officeBench = { frames: [] };
  });
  page.on("pageerror", (e) => console.error("page error:", e.message));
  await page.goto(`http://localhost:${port}/?stress`);
  // Everyone walks in from the door first; measure the busy office after that.
  await page.waitForTimeout(12_000);
  await page.evaluate(() => {
    window.__officeBench.frames.length = 0;
    window.__officeBench.started = performance.now();
  });
  await page.waitForTimeout(seconds * 1000);
  const result = await page.evaluate(() => {
    const frames = [...window.__officeBench.frames].sort((a, b) => a - b);
    const elapsed = (performance.now() - window.__officeBench.started) / 1000;
    const stats = document.querySelector(".header-stats");
    const agents = stats ? [...stats.children].map((c) => c.textContent).join(" · ") : "";
    const pick = (q) => frames[Math.min(frames.length - 1, Math.floor(frames.length * q))] ?? 0;
    return {
      frames: frames.length,
      fps: frames.length / elapsed,
      mean: frames.reduce((s, v) => s + v, 0) / Math.max(1, frames.length),
      p50: pick(0.5),
      p95: pick(0.95),
      max: frames[frames.length - 1] ?? 0,
      header: agents,
    };
  });
  const f = (n) => n.toFixed(2);
  console.log(`office bench · ${width}x${height} @${dpr}x · ${seconds}s · ${result.header}`);
  console.log(`frames drawn ${result.frames} (${result.fps.toFixed(1)} fps)`);
  console.log(`frame cost ms: mean ${f(result.mean)} · p50 ${f(result.p50)} · p95 ${f(result.p95)} · max ${f(result.max)}`);
  const ok = result.p95 < 16.7 && result.fps > 50;
  console.log(ok ? "OFFICE BENCH: fits 60 fps" : "OFFICE BENCH: too slow for 60 fps on this machine");
  process.exitCode = ok ? 0 : 1;
} finally {
  await browser?.close();
  stop();
}
