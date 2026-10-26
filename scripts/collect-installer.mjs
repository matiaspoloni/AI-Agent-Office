// Copies the installer produced by `tauri build` to dist-installer/ under a
// stable name for releases: AgentOfficeSetup.exe (Windows, NSIS) or
// AgentOffice.dmg (macOS).
import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { arch } from "node:os";
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
collectUpdate(candidates[0], releaseDirs);

/** Signed updater payload, when this build was made with TAURI_SIGNING_PRIVATE_KEY. */
function collectUpdate(installer, releaseDirs) {
  if (!process.env.TAURI_SIGNING_PRIVATE_KEY) return;
  const version = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8")).version;
  const bundles = releaseDirs.map((dir) => join(dir, "bundle")).filter((dir) => existsSync(dir));
  const files = bundles.flatMap((dir) => walk(dir));
  /** @type {Record<string, { signature: string, url: string }>} */
  const platforms = {};

  if (process.platform === "darwin") {
    const archives = files.filter((f) => f.endsWith(".app.tar.gz")).sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs);
    const archive = archives[0];
    if (!archive) return requireSignature("macOS update archive");
    const signature = readSignature(archive);
    const name = `AgentOffice-${version}.app.tar.gz`;
    copyFileSync(archive, join("dist-installer", name));
    const keys = archive.includes("universal-apple-darwin")
      ? ["darwin-aarch64", "darwin-x86_64"]
      : [arch() === "arm64" ? "darwin-aarch64" : "darwin-x86_64"];
    for (const key of keys) platforms[key] = { signature, url: name };
    writeManifest("latest-macos.json", version, platforms);
    return;
  }

  const signature = readSignature(installer);
  const name = `AgentOfficeSetup-${version}.exe`;
  copyFileSync(installer, join("dist-installer", name));
  const key = arch() === "arm64" ? "windows-aarch64" : "windows-x86_64";
  platforms[key] = { signature, url: name };
  writeManifest("latest-windows.json", version, platforms);
}

function requireSignature(what) {
  if (process.env.TAURI_SIGNING_PRIVATE_KEY) {
    console.error(`${what} is missing; the update key was set, so the build must produce a signature.`);
    process.exit(1);
  }
}

function readSignature(artifact) {
  const sig = `${artifact}.sig`;
  if (!existsSync(sig)) {
    requireSignature(sig);
    return "";
  }
  return readFileSync(sig, "utf8").trim();
}

function writeManifest(file, version, platforms) {
  if (Object.values(platforms).some((p) => !p.signature)) return;
  const manifest = {
    version,
    notes: `Build ${version}`,
    pub_date: new Date().toISOString(),
    platforms,
  };
  writeFileSync(join("dist-installer", file), JSON.stringify(manifest, null, 2));
  console.log(`Update manifest ready: dist-installer/${file}`);
}

function walk(dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) out.push(...walk(path));
    else out.push(path);
  }
  return out;
}
