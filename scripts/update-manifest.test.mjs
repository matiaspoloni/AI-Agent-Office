import assert from "node:assert/strict";
import test from "node:test";
import { mergeUpdateManifests } from "./update-manifest.mjs";

const release = { repo: "matiaspoloni/AI-Agent-Office", tag: "continuous" };

test("merges both platforms into absolute download URLs", () => {
  const merged = mergeUpdateManifests(
    [
      {
        version: "0.1.40",
        notes: "Build 0.1.40",
        platforms: {
          "darwin-aarch64": { signature: "mac-sig", url: "AgentOffice-0.1.40.app.tar.gz" },
          "darwin-x86_64": { signature: "mac-sig", url: "AgentOffice-0.1.40.app.tar.gz" },
        },
      },
      {
        version: "0.1.40",
        platforms: { "windows-x86_64": { signature: "win-sig", url: "AgentOfficeSetup-0.1.40.exe" } },
      },
    ],
    release,
  );
  assert.equal(merged.version, "0.1.40");
  assert.equal(
    merged.platforms["darwin-aarch64"].url,
    "https://github.com/matiaspoloni/AI-Agent-Office/releases/download/continuous/AgentOffice-0.1.40.app.tar.gz",
  );
  assert.equal(merged.platforms["windows-x86_64"].signature, "win-sig");
});

test("waits until Windows and macOS from the same build both exist", () => {
  const mac = {
    version: "0.1.41",
    platforms: { "darwin-aarch64": { signature: "mac-sig", url: "AgentOffice-0.1.41.app.tar.gz" } },
  };
  assert.equal(mergeUpdateManifests([mac], release), null);
});

test("refuses to publish when the two platforms were built as different versions", () => {
  assert.throws(
    () =>
      mergeUpdateManifests(
        [
          { version: "0.1.41", platforms: { "darwin-aarch64": { signature: "a", url: "a.tar.gz" } } },
          { version: "0.1.42", platforms: { "windows-x86_64": { signature: "b", url: "b.exe" } } },
        ],
        release,
      ),
    /disagree/,
  );
});
