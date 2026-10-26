// Give a published build a version newer than the last one. The files in git
// stay at 0.1.0; CI sets AGENT_OFFICE_BUILD_NUMBER to the workflow run number
// after the tests, so the shipped app is 0.1.<run> and the updater can tell
// the builds apart. A real v* tag is not stamped: 0.2.0 stays ahead of 0.1.N.
import { readFileSync, writeFileSync } from "node:fs";

const run = process.env.AGENT_OFFICE_BUILD_NUMBER ?? "";
if (!/^[1-9]\d*$/.test(run)) {
  console.log("AGENT_OFFICE_BUILD_NUMBER is unset; leaving the version at 0.1.0.");
  process.exit(0);
}
const version = `0.1.${run}`;

const cargoPath = "Cargo.toml";
const cargo = readFileSync(cargoPath, "utf8");
const stampedCargo = cargo.replace(/(\[workspace\.package\][^\[]*?^version = ")[^"]+(")/m, `$1${version}$2`);
if (stampedCargo === cargo) {
  console.error("Could not find [workspace.package] version in Cargo.toml");
  process.exit(1);
}
writeFileSync(cargoPath, stampedCargo);

for (const path of ["src-tauri/tauri.conf.json", "package.json"]) {
  const json = JSON.parse(readFileSync(path, "utf8"));
  json.version = version;
  writeFileSync(path, `${JSON.stringify(json, null, path.endsWith("tauri.conf.json") ? 2 : 2)}\n`);
}
console.log(`Stamped published build ${version}`);
