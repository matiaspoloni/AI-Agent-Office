// Disk image smoke test (macOS): mounts dist-installer/AgentOffice.dmg and
// checks the app in it (identifier, version, minimum macOS, both processors,
// signature); checks that hooks are refused while the app runs from the
// image; installs it the way a user does (a copy in an Applications folder);
// runs the copy's self-check; installs and removes the Claude hooks with it
// (the user's own settings stay); and, in CI, opens it like Finder does to
// see it start and adopt the login shell's environment.
//
//   node scripts/dmg-smoke.mjs --yes
//
// Claude Code, Codex CLI and Agent Office data are redirected to a temporary
// folder for everything but the last step (CI only: an app opened by Finder
// gets the system's environment, so it uses the default data folder).
import { spawn, spawnSync } from "node:child_process";
import {
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";

if (process.platform !== "darwin") {
  console.log("Disk image smoke test: macOS only, skipped.");
  process.exit(0);
}
if (!process.env.CI && !process.argv.includes("--yes")) {
  console.error("This mounts the disk image and runs the app from it. Run with --yes (CI does).");
  process.exit(2);
}
const dmg = join("dist-installer", "AgentOffice.dmg");
if (!existsSync(dmg)) {
  console.error(`${dmg} is missing: run npm run build first.`);
  process.exit(2);
}

const home = mkdtempSync(join(tmpdir(), "ao-dmg-"));
const apps = join(home, "Applications");
const env = {
  ...process.env,
  CLAUDE_CONFIG_DIR: join(home, "claude"),
  CODEX_HOME: join(home, "codex"),
  AGENT_OFFICE_DATA_DIR: join(home, "data"),
};
mkdirSync(env.CLAUDE_CONFIG_DIR, { recursive: true });
mkdirSync(env.CODEX_HOME, { recursive: true });
const settingsFile = join(env.CLAUDE_CONFIG_DIR, "settings.json");
writeFileSync(
  settingsFile,
  JSON.stringify({ model: "opus", hooks: { Stop: [{ hooks: [{ type: "command", command: "notify-me" }] }] } }, null, 2),
);
const version = JSON.parse(readFileSync("package.json", "utf8")).version;

let failures = 0;
const check = (ok, what) => {
  console.log(`${ok ? "ok  " : "FAIL"} ${what}`);
  if (!ok) failures++;
};
const run = (file, args, options = {}) => spawnSync(file, args, { env, encoding: "utf8", timeout: 180_000, ...options });
const settings = () => JSON.parse(readFileSync(settingsFile, "utf8"));
const hasOurHook = () => JSON.stringify(settings()).includes("agent-office");
const exe = (app) => join(app, "Contents", "MacOS", "agent-office");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function until(condition, what, seconds) {
  for (let i = 0; i < seconds * 4; i++) {
    if (condition()) return true;
    await sleep(250);
  }
  check(false, `${what} (timed out)`);
  return false;
}

// 1. The disk image and the app in it, mounted where macOS mounts it for the
//    user (/Volumes/…).
const attached = run("hdiutil", ["attach", "-nobrowse", "-readonly", "-noautoopen", "-plist", dmg]);
check(attached.status === 0, "disk image mounts");
const entities = JSON.parse(
  run("plutil", ["-convert", "json", "-o", "-", "-"], { input: attached.stdout }).stdout || "{}",
)["system-entities"];
const mount = (entities ?? []).map((e) => e["mount-point"]).find(Boolean) ?? "";
check(mount.startsWith("/Volumes/"), `mounted at ${mount}`);
const mountedApp = join(mount, "Agent Office.app");
check(existsSync(mountedApp), "Agent Office.app in the disk image");
check(existsSync(join(mount, "Applications")) && lstatSync(join(mount, "Applications")).isSymbolicLink(), "Applications shortcut next to it");
const plist = (app, key) =>
  run("plutil", ["-extract", key, "raw", "-o", "-", join(app, "Contents", "Info.plist")]).stdout.trim();
check(plist(mountedApp, "CFBundleIdentifier") === "com.agentoffice.desktop", "bundle identifier");
check(plist(mountedApp, "CFBundleShortVersionString") === version, `version ${version}`);
check(plist(mountedApp, "LSMinimumSystemVersion") === "11.0", "macOS 11 or later");
const archs = run("lipo", ["-archs", exe(mountedApp)]).stdout.trim();
check(archs.includes("arm64") && archs.includes("x86_64"), `one app for Apple silicon and Intel Macs (${archs})`);
check(
  run("codesign", ["--verify", "--deep", "--strict", "--verbose=2", mountedApp], { stdio: "inherit" }).status === 0,
  "signature intact (ad hoc)",
);

// 2. Running from the image, hooks would point to a path that disappears.
const refused = run(exe(mountedApp), ["integrations", "install", "claude"]);
check(refused.status === 1 && refused.stdout.includes("disk image"), "hooks are refused while running from the disk image");
check(!hasOurHook(), "Claude settings untouched");

// 3. Install like a user: copy to an Applications folder, eject the image.
mkdirSync(apps);
check(run("ditto", [mountedApp, join(apps, "Agent Office.app")]).status === 0, "app copied to Applications");
run("hdiutil", ["detach", mount]);
const app = join(apps, "Agent Office.app");

// 4. The installed copy runs (headless self-check, no window).
check(run(exe(app), ["--smoke-test"], { stdio: ["ignore", "ignore", "inherit"] }).status === 0, "installed app passes its smoke test");
const report = JSON.parse(readFileSync(join(env.AGENT_OFFICE_DATA_DIR, "smoke-report.json"), "utf8"));
check(report.notifications.supported === true, "the app bundle can show notifications");
check(!report.hooks.relayWarning, "no location warning once installed");
check(
  report.paths.find((p) => p.label === "Claude Code user settings")?.path === settingsFile,
  "Diagnostics shows the Claude settings it will edit",
);

// 5. Hooks: installed with the copy, run as Claude runs them, removed again.
check(run(exe(app), ["integrations", "install", "claude"]).status === 0, "Claude hooks installed");
check(
  [exe(app), realpathSync(exe(app))].some((path) => JSON.stringify(settings()).includes(path)),
  "hooks point to the installed app",
);
const ours = settings()
  .hooks.Stop.flatMap((group) => group.hooks)
  .find((h) => String(h.command).includes("agent-office"));
// Exec form (program + args, no shell), with Agent Office not running: the
// relay lets Claude carry on.
const hook = run(ours.command, ours.args, { input: JSON.stringify({ hook_event_name: "Stop", session_id: "dmg-smoke" }), timeout: 15_000 });
check(hook.status === 0, "a hook run with Agent Office closed lets Claude carry on");
check(run(exe(app), ["integrations", "uninstall"]).status === 0, "Claude hooks removed");
check(!hasOurHook(), "no Agent Office entries left");
check(settings().model === "opus" && JSON.stringify(settings()).includes("notify-me"), "the user's own settings kept");
check(
  readdirSync(env.CLAUDE_CONFIG_DIR).some((f) => f.startsWith("settings.json.agent-office-backup-")),
  "backup of the settings file kept",
);

// 6. CI only: the app with its window. First started directly (its output is
//    captured: a crash shows here), then opened like Finder does (by launchd,
//    with its minimal environment), where it adopts the login shell's
//    environment.
const readLogs = (dir) =>
  existsSync(dir)
    ? readdirSync(dir)
        .filter((f) => f.startsWith("app.log"))
        .map((f) => readFileSync(join(dir, f), "utf8"))
        .join("\n")
    : "";
const tail = (text, lines = 40) => text.split("\n").slice(-lines).join("\n");
function crashReports() {
  const dir = join(homedir(), "Library", "Logs", "DiagnosticReports");
  if (!existsSync(dir)) return "(no DiagnosticReports folder)";
  const reports = readdirSync(dir).filter((f) => f.toLowerCase().includes("agent-office"));
  return reports.length
    ? reports.map((f) => `--- ${f}\n${readFileSync(join(dir, f), "utf8").slice(0, 6000)}`).join("\n")
    : "(no crash report)";
}
const running = () => run("pgrep", ["-f", exe(app)]).stdout.trim().split("\n").filter(Boolean).map(Number);

if (process.env.CI) {
  const direct = spawn(exe(app), [], { env: { ...env, RUST_BACKTRACE: "1" }, stdio: ["ignore", "pipe", "pipe"] });
  let output = "";
  direct.stdout.on("data", (d) => (output += d));
  direct.stderr.on("data", (d) => (output += d));
  const directLogs = join(env.AGENT_OFFICE_DATA_DIR, "logs");
  await until(() => readLogs(directLogs).includes("starting Agent Office") || direct.exitCode !== null, "window app writes its log", 30);
  await sleep(8000);
  const alive = direct.exitCode === null && direct.signalCode === null;
  check(alive, "window app keeps running (started directly)");
  if (!alive) {
    console.log(`exit ${direct.exitCode} signal ${direct.signalCode}\n${tail(output, 60)}`);
    console.log(tail(readLogs(directLogs)));
    console.log(crashReports());
  }
  direct.kill("SIGTERM");
  await until(() => direct.exitCode !== null || direct.signalCode !== null, "directly started app quit", 30);

  // `open` passes its own environment on to the app, so it is given only
  // what launchd gives an app opened from Finder (and its own data folder).
  const finderData = join(home, "data-finder");
  const finderEnv = {
    HOME: process.env.HOME,
    USER: process.env.USER,
    LOGNAME: process.env.LOGNAME ?? process.env.USER,
    PATH: "/usr/bin:/bin:/usr/sbin:/sbin",
    TMPDIR: process.env.TMPDIR,
    AGENT_OFFICE_DATA_DIR: finderData,
  };
  // Where its log is, whether or not this macOS passes the environment on.
  const logDirs = [join(finderData, "logs"), join(homedir(), "Library", "Application Support", "AgentOffice", "logs")];
  const finderLogs = () => logDirs.map(readLogs).join("\n");
  check(run("open", ["-n", app], { env: finderEnv }).status === 0, "opened like Finder does");
  const adopted = await until(
    () => /adopted the login shell's environment.*variables=[1-9]/.test(finderLogs()),
    "login shell environment adopted",
    60,
  );
  check(finderLogs().includes("starting Agent Office"), "app started");
  if (!adopted) {
    console.log(`running: ${JSON.stringify(running())}`);
    for (const dir of logDirs) console.log(`log folder ${dir}: ${existsSync(dir) ? readdirSync(dir).join(", ") : "missing"}`);
    console.log(tail(finderLogs()));
    console.log(crashReports());
  }
  // Stop only the app opened here.
  for (const pid of running()) process.kill(pid, "SIGTERM");
  await until(() => running().length === 0, "app quit", 30);
}

rmSync(home, { recursive: true, force: true });
console.log(failures ? `DISK IMAGE SMOKE TEST FAILED (${failures})` : "DISK IMAGE SMOKE TEST PASSED");
process.exit(failures ? 1 : 0);
