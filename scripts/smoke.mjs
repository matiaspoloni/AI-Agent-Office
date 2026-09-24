// Cross-platform smoke test: builds the desktop binary, runs it headless with
// `--smoke-test` against a throw-away data folder, and checks the report.
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const release = process.argv.includes("--release");
const dataDir = mkdtempSync(join(tmpdir(), "agent-office-smoke-"));
const cargoArgs = ["run", "-q", "-p", "agent-office", ...(release ? ["--release"] : []), "--", "--smoke-test"];

console.log(`> cargo ${cargoArgs.join(" ")}  (AGENT_OFFICE_DATA_DIR=${dataDir})`);
const result = spawnSync("cargo", cargoArgs, {
  stdio: ["ignore", "pipe", "inherit"],
  env: { ...process.env, AGENT_OFFICE_DATA_DIR: dataDir },
  shell: process.platform === "win32",
});

let ok = result.status === 0;
try {
  const report = JSON.parse(readFileSync(join(dataDir, "smoke-report.json"), "utf8"));
  const providers = report.providers.map((p) => `${p.provider.descriptor.id}:${p.installation.installed ? "installed" : "missing"}`);
  console.log(`database ok=${report.database.ok} · providers ${providers.join(", ")} · git ${report.git.installed ? "yes" : "no"}`);
  ok = ok && report.database.ok && report.providers.length >= 4;
} catch (error) {
  console.error(`Could not read smoke report: ${error.message}`);
  ok = false;
}
rmSync(dataDir, { recursive: true, force: true });
console.log(ok ? "SMOKE TEST PASSED" : "SMOKE TEST FAILED");
process.exit(ok ? 0 : 1);
