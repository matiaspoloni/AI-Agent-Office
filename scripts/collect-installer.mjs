// Copies the installer produced by `tauri build` to dist-installer/ under a
// stable name for releases: AgentOfficeSetup.exe (Windows, NSIS) or
// AgentOffice.dmg (macOS).
import { copyFileSync, existsSync, mkdirSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const bundles =
  process.platform === "darwin"
    ? { kind: "dmg", ending: ".dmg", name: "AgentOffice.dmg" }
    : { kind: "nsis", ending: "-setup.exe", name: "AgentOfficeSetup.exe" };

// target/release or, for a universal macOS build, target/<triple>/release.
const releaseDirs = [join("target", "release")];
if (existsSync("target")) {
  for (const entry of readdirSync("target")) releaseDirs.push(join("target", entry, "release"));
}
// The newest one, should earlier builds have left others behind.
const candidates = releaseDirs
  .map((dir) => join(dir, "bundle", bundles.kind))
  .filter((dir) => existsSync(dir))
  .flatMap((dir) => readdirSync(dir).filter((f) => f.toLowerCase().endsWith(bundles.ending)).map((f) => join(dir, f)))
  .sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs);
if (candidates.length === 0) {
  console.log(`No ${bundles.kind} bundle found (not a Windows or macOS build); nothing to collect.`);
  process.exit(0);
}
mkdirSync("dist-installer", { recursive: true });
const target = join("dist-installer", bundles.name);
copyFileSync(candidates[0], target);
console.log(`Installer ready: ${target} (from ${candidates[0]})`);
