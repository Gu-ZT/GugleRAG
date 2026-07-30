import fs from "node:fs";
import { artifactFiles, readChangelog, renderReleaseNotes } from "./release-lib.mjs";

function argument(name) {
  const index = process.argv.indexOf(name);
  if (index < 0 || !process.argv[index + 1]) throw new Error(`missing ${name}`);
  return process.argv[index + 1];
}

const version = argument("--version");
const changelogVersion = argument("--changelog-version");
const artifacts = argument("--artifacts");
const output = argument("--output");
const notes = renderReleaseNotes({
  version,
  english: readChangelog("CHANGELOG.md", changelogVersion),
  chinese: readChangelog("CHANGELOG.zh-CN.md", changelogVersion),
  files: artifactFiles(artifacts)
});
fs.writeFileSync(output, notes);
console.log(`wrote ${output}`);
