import fs from "node:fs";
import { resolveReleaseIdentity } from "./release-lib.mjs";

function argument(name) {
  const index = process.argv.indexOf(name);
  if (index < 0 || !process.argv[index + 1]) throw new Error(`missing ${name}`);
  return process.argv[index + 1];
}

const frontend = JSON.parse(fs.readFileSync("frontend/package.json", "utf8"));
const identity = resolveReleaseIdentity({
  version: frontend.version,
  refType: argument("--ref-type"),
  refName: argument("--ref-name"),
  runNumber: argument("--run-number")
});
const output = argument("--github-output");
const values = {
  tag: identity.tag,
  version: identity.version,
  changelog_version: identity.changelogVersion,
  prerelease: String(identity.prerelease)
};
fs.appendFileSync(output, Object.entries(values).map(([key, value]) => `${key}=${value}\n`).join(""));
console.log(JSON.stringify(identity));
