// `npm run build`: builds the desktop app and its Windows installer, then
// copies the installer to dist-installer/AgentOfficeSetup.exe.
//
// Code signing is optional. It is switched on only when a certificate is
// available to signtool (e.g. imported into the Windows certificate store in
// CI) and its SHA-1 thumbprint is given:
//
//   AGENT_OFFICE_SIGN_THUMBPRINT=<thumbprint>
//   AGENT_OFFICE_SIGN_TIMESTAMP_URL=<url>   (default http://timestamp.digicert.com)
//
// Without it the installer is built unsigned (Windows SmartScreen will warn
// on first run; see docs/DEVELOPMENT.md).
import { spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const tauri = join("node_modules", "@tauri-apps", "cli", "tauri.js");
const args = [tauri, "build"];

const thumbprint = (process.env.AGENT_OFFICE_SIGN_THUMBPRINT ?? "").replace(/\s+/g, "");
if (thumbprint) {
  if (!/^[0-9a-fA-F]{40}$/.test(thumbprint)) {
    console.error("AGENT_OFFICE_SIGN_THUMBPRINT must be a 40-character SHA-1 thumbprint.");
    process.exit(2);
  }
  const config = {
    bundle: {
      windows: {
        certificateThumbprint: thumbprint,
        digestAlgorithm: "sha256",
        timestampUrl: process.env.AGENT_OFFICE_SIGN_TIMESTAMP_URL || "http://timestamp.digicert.com",
      },
    },
  };
  mkdirSync("target", { recursive: true });
  const file = resolve("target", "tauri.signing.conf.json");
  writeFileSync(file, JSON.stringify(config, null, 2));
  args.push("--config", file);
  console.log("Signing with the certificate", thumbprint.slice(0, 8) + "…");
} else {
  console.log("Building an unsigned installer (set AGENT_OFFICE_SIGN_THUMBPRINT to sign).");
}

const built = spawnSync(process.execPath, args, { stdio: "inherit" });
if (built.status !== 0) process.exit(built.status ?? 1);

const collected = spawnSync(process.execPath, [join("scripts", "collect-installer.mjs")], { stdio: "inherit" });
process.exit(collected.status ?? 1);
