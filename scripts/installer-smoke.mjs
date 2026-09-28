// Installer smoke test (Windows): installs dist-installer/AgentOfficeSetup.exe
// silently, checks what it put on the machine, runs the installed app, then
// uninstalls it and checks that it cleaned up — including Agent Office's
// hook entries in the Claude Code settings, while the user's own entries stay.
//
// It really installs the app for the current Windows user, so it only runs in
// CI or when asked explicitly:
//
//   node scripts/installer-smoke.mjs --yes
//
// Claude Code, Codex CLI and Agent Office data are redirected to a temporary
// folder (CLAUDE_CONFIG_DIR, CODEX_HOME, AGENT_OFFICE_DATA_DIR), so no real
// settings are touched.
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

if (process.platform !== "win32") {
  console.log("Installer smoke test: Windows only, skipped.");
  process.exit(0);
}
if (!process.env.CI && !process.argv.includes("--yes")) {
  console.error("This installs and uninstalls Agent Office for the current user. Run with --yes (CI does).");
  process.exit(2);
}

const installer = join("dist-installer", "AgentOfficeSetup.exe");
if (!existsSync(installer)) {
  console.error(`${installer} is missing: run npm run build first.`);
  process.exit(2);
}

const home = mkdtempSync(join(tmpdir(), "ao-installer-"));
const env = {
  ...process.env,
  CLAUDE_CONFIG_DIR: join(home, "claude"),
  CODEX_HOME: join(home, "codex"),
  AGENT_OFFICE_DATA_DIR: join(home, "data"),
};
mkdirSync(env.CLAUDE_CONFIG_DIR, { recursive: true });
mkdirSync(env.CODEX_HOME, { recursive: true });
const settingsFile = join(env.CLAUDE_CONFIG_DIR, "settings.json");
const userSettings = {
  model: "opus",
  hooks: { Stop: [{ hooks: [{ type: "command", command: "notify-me" }] }] },
};
writeFileSync(settingsFile, JSON.stringify(userSettings, null, 2));

const localAppData = process.env.LOCALAPPDATA;
const appData = process.env.APPDATA;
const installDir = join(localAppData, "Agent Office");
const exe = join(installDir, "agent-office.exe");
const uninstaller = join(installDir, "uninstall.exe");
const startMenu = join(appData, "Microsoft", "Windows", "Start Menu", "Programs", "Agent Office", "Agent Office.lnk");
const desktop = join(process.env.USERPROFILE, "Desktop", "Agent Office.lnk");
const uninstallKey = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Agent Office";

let failures = 0;
const check = (ok, what) => {
  console.log(`${ok ? "ok  " : "FAIL"} ${what}`);
  if (!ok) failures++;
};
const run = (file, args) => spawnSync(file, args, { env, stdio: "inherit", timeout: 180_000 });
const registryHas = (key) => spawnSync("reg", ["query", key], { stdio: "ignore" }).status === 0;
const settings = () => JSON.parse(readFileSync(settingsFile, "utf8"));
const hasOurHook = () => JSON.stringify(settings()).includes("agent-office");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function until(condition, what, seconds = 90) {
  for (let i = 0; i < seconds * 4; i++) {
    if (condition()) return true;
    await sleep(250);
  }
  check(false, `${what} (timed out)`);
  return false;
}

// 1. Install.
check(run(installer, ["/S"]).status === 0, "installer exits with 0");
check(existsSync(exe), `app installed at ${exe}`);
check(existsSync(uninstaller), "uninstaller installed");
check(existsSync(startMenu), "Start Menu shortcut");
check(existsSync(desktop), "desktop shortcut");
check(registryHas(uninstallKey), "listed in Installed apps (uninstall key)");
const version = execFileSync("reg", ["query", uninstallKey, "/v", "DisplayVersion"], { encoding: "utf8" });
const expected = JSON.parse(readFileSync("package.json", "utf8")).version;
check(version.includes(expected), `version ${expected} registered`);

// 2. The installed app runs (headless self-check, no window).
check(run(exe, ["--smoke-test"]).status === 0, "installed app passes its smoke test");
check(existsSync(join(env.AGENT_OFFICE_DATA_DIR, "smoke-report.json")), "smoke report written");

// 3. Hooks: installed by the app, removed by the uninstaller.
check(run(exe, ["integrations", "install", "claude"]).status === 0, "Claude hooks installed");
check(hasOurHook(), "Agent Office entries in the Claude settings");

check(run(uninstaller, ["/S"]).status === 0, "uninstaller starts");
// The NSIS uninstaller copies itself to %TEMP% and returns at once: wait.
await until(() => !existsSync(exe) && !registryHas(uninstallKey), "app removed");
check(!existsSync(exe), "program files removed");
check(!existsSync(startMenu), "Start Menu shortcut removed");
check(!existsSync(desktop), "desktop shortcut removed");
check(!registryHas(uninstallKey), "uninstall key removed");
check(!hasOurHook(), "Agent Office hook entries removed from the Claude settings");
const after = settings();
check(after.model === "opus" && JSON.stringify(after).includes("notify-me"), "the user's own settings kept");
check(
  readdirSync(env.CLAUDE_CONFIG_DIR).some((f) => f.startsWith("settings.json.agent-office-backup-")),
  "backup of the settings file kept",
);
check(existsSync(join(env.AGENT_OFFICE_DATA_DIR, "integrations-removed.json")), "removed integrations noted");

// 4. Installing again right away (what an upgrade does after the old
//    uninstaller) brings the hooks back; then clean up.
check(run(installer, ["/S"]).status === 0, "reinstall exits with 0");
check(hasOurHook(), "hooks restored after reinstalling");
run(uninstaller, ["/S"]);
await until(() => !existsSync(exe), "second uninstall");

rmSync(home, { recursive: true, force: true });
console.log(failures ? `INSTALLER SMOKE TEST FAILED (${failures})` : "INSTALLER SMOKE TEST PASSED");
process.exit(failures ? 1 : 0);
