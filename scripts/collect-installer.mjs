// Copies the NSIS installer produced by `tauri build` to
// dist-installer/AgentOfficeSetup.exe (stable name for releases).
import { copyFileSync, existsSync, mkdirSync, readdirSync } from "node:fs";
import { join } from "node:path";

const bundleDir = join("target", "release", "bundle", "nsis");
if (!existsSync(bundleDir)) {
  console.log(`No NSIS bundle found in ${bundleDir} (not a Windows build); nothing to collect.`);
  process.exit(0);
}
const installer = readdirSync(bundleDir).find((f) => f.toLowerCase().endsWith("-setup.exe"));
if (!installer) {
  console.error(`No *-setup.exe in ${bundleDir}`);
  process.exit(1);
}
mkdirSync("dist-installer", { recursive: true });
const target = join("dist-installer", "AgentOfficeSetup.exe");
copyFileSync(join(bundleDir, installer), target);
console.log(`Installer ready: ${target}`);
