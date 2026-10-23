// `npm run build`: builds the desktop app and its installer, then copies it
// to dist-installer/:
//
// * Windows: AgentOfficeSetup.exe (NSIS).
// * macOS: AgentOffice.dmg with Agent Office.app, one app for Apple silicon
//   and Intel Macs (needs both Rust targets; this script adds them with
//   rustup, or builds for this Mac's processor only without rustup).
//
// Windows code signing is optional. It is switched on only when a
// certificate is available to signtool (e.g. imported into the Windows
// certificate store in CI) and its SHA-1 thumbprint is given:
//
//   AGENT_OFFICE_SIGN_THUMBPRINT=<thumbprint>
//   AGENT_OFFICE_SIGN_TIMESTAMP_URL=<url>   (default http://timestamp.digicert.com)
//
// Without it the installer is built unsigned (Windows SmartScreen will warn
// on first run; see docs/DEVELOPMENT.md). The macOS app is signed ad hoc
// (no Apple certificate); Tauri's own APPLE_SIGNING_IDENTITY /
// APPLE_CERTIFICATE variables sign it with a Developer ID instead.
import { spawnSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const tauri = join("node_modules", "@tauri-apps", "cli", "tauri.js");
const args = [tauri, "build"];

if (process.platform === "darwin") {
  const targets = spawnSync("rustup", ["target", "add", "aarch64-apple-darwin", "x86_64-apple-darwin"], {
    stdio: "inherit",
  });
  if (targets.status === 0) {
    args.push("--target", "universal-apple-darwin");
  } else {
    console.log("rustup is not available: building for this Mac's processor only.");
  }
  args.push("--bundles", "app,dmg");
}

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
