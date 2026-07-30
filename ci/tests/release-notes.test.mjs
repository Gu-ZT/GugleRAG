import assert from "node:assert/strict";
import test from "node:test";
import {
  classifyArtifacts,
  extractChangelogSection,
  releaseTargets,
  renderReleaseNotes
} from "../release-lib.mjs";

function files(version) {
  return releaseTargets.flatMap((target) => {
    const archive = `guglerag-v${version}-${target.platform}-${target.arch}.${target.format}`;
    return [archive, `${archive}.sha256`];
  });
}

test("extracts one exact changelog version", () => {
  const source = "# Changes\n\n## [1.2.0]\n\n- New\n\n## [1.1.0]\n\n- Old\n";
  assert.equal(extractChangelogSection(source, "1.2.0"), "- New");
});

test("rejects unknown or missing release artifacts", () => {
  assert.throws(() => classifyArtifacts([...files("1.2.0"), "unknown.bin"], "1.2.0"));
  assert.throws(() => classifyArtifacts(files("1.2.0").slice(1), "1.2.0"));
});

test("renders every supported platform and both languages", () => {
  const notes = renderReleaseNotes({
    version: "1.2.0",
    english: "- English change",
    chinese: "- 中文变更",
    files: files("1.2.0")
  });
  assert.match(notes, /Linux x64/);
  assert.match(notes, /Linux ARM64/);
  assert.match(notes, /Windows x64/);
  assert.match(notes, /Windows ARM64/);
  assert.match(notes, /macOS Apple Silicon/);
  assert.match(notes, /macOS Intel/);
  assert.match(notes, /English change/);
  assert.match(notes, /中文变更/);
});
