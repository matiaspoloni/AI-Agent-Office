// Merge the macOS and Windows update manifests into one latest.json whose
// download URLs point at this GitHub release. One version covers every
// platform, so a manifest is published only when both platforms from the same
// build are present. Otherwise the previous latest.json stays.
import { existsSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";

/**
 * @param {Array<{ version: string, notes?: string, platforms: Record<string, { signature: string, url: string }> }>} partials
 * @param {{ repo: string, tag: string }} release
 */
export function mergeUpdateManifests(partials, release) {
  if (partials.length === 0) return null;
  const versions = [...new Set(partials.map((p) => p.version))];
  if (versions.length !== 1) {
    throw new Error(`Update manifests disagree on the version: ${versions.join(", ")}`);
  }
  /** @type {Record<string, { signature: string, url: string }>} */
  const platforms = {};
  for (const partial of partials) {
    for (const [key, platform] of Object.entries(partial.platforms ?? {})) {
      if (!platform.signature || !platform.url) throw new Error(`Update platform ${key} is incomplete`);
      const name = platform.url.split("/").pop();
      platforms[key] = {
        signature: platform.signature,
        url: `https://github.com/${release.repo}/releases/download/${release.tag}/${name}`,
      };
    }
  }
  const hasDarwin = Object.keys(platforms).some((key) => key.startsWith("darwin-"));
  if (!hasDarwin || !platforms["windows-x86_64"]) return null;
  return {
    version: versions[0],
    notes: partials.find((p) => p.notes)?.notes ?? `Build ${versions[0]}`,
    pub_date: new Date().toISOString(),
    platforms,
  };
}

function readPartial(dir, name) {
  const path = join(dir, name);
  if (!existsSync(path)) return null;
  return JSON.parse(readFileSync(path, "utf8"));
}

const isMain = process.argv[1] && import.meta.url.endsWith(process.argv[1].replaceAll("\\", "/").split("/").pop());
if (isMain && process.argv[1].endsWith("update-manifest.mjs")) {
  const dir = process.argv[2] ?? "out";
  const tag = process.argv[3];
  const repo = process.env.GITHUB_REPOSITORY;
  if (!tag || !repo) {
    console.error("Usage: GITHUB_REPOSITORY=owner/repo node scripts/update-manifest.mjs <dir> <tag>");
    process.exit(1);
  }
  const partials = ["latest-macos.json", "latest-windows.json"].map((name) => readPartial(dir, name)).filter(Boolean);
  const merged = mergeUpdateManifests(partials, { repo, tag });
  for (const name of ["latest-macos.json", "latest-windows.json"]) {
    const path = join(dir, name);
    if (existsSync(path)) unlinkSync(path);
  }
  if (!merged) {
    console.log("No full update manifest (macOS and Windows from the same build are both required).");
  } else {
    writeFileSync(join(dir, "latest.json"), `${JSON.stringify(merged, null, 2)}\n`);
    console.log(`Update manifest ${merged.version} for ${Object.keys(merged.platforms).join(", ")}`);
  }
}
