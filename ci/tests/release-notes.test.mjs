import assert from "node:assert/strict";
import test from "node:test";
import {
  classifyArtifacts,
  extractChangelogSection,
  releaseTargets,
  resolveReleaseIdentity,
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

test("resolves main builds as unique prereleases", () => {
  assert.deepEqual(
    resolveReleaseIdentity({
      version: "1.2.0",
      refType: "branch",
      refName: "main",
      runNumber: "42"
    }),
    {
      tag: "v1.2.0-dev.42",
      version: "1.2.0-dev.42",
      changelogVersion: "1.2.0",
      prerelease: true
    }
  );
});

test("resolves exact version tags as stable releases", () => {
  assert.deepEqual(
    resolveReleaseIdentity({
      version: "1.2.0",
      refType: "tag",
      refName: "v1.2.0",
      runNumber: "42"
    }),
    {
      tag: "v1.2.0",
      version: "1.2.0",
      changelogVersion: "1.2.0",
      prerelease: false
    }
  );
  assert.throws(() =>
    resolveReleaseIdentity({
      version: "1.2.0",
      refType: "tag",
      refName: "v1.2.1",
      runNumber: "42"
    })
  );
});

test("renders every supported platform and both languages", () => {
  const notes = renderReleaseNotes({
    version: "1.2.0",
    english: "- English change",
    chinese: "- 中文变更",
    files: files("1.2.0"),
    repository: "guglerag/guglerag"
  });
  assert.match(notes, /Linux x64/);
  assert.match(notes, /Linux ARM64/);
  assert.match(notes, /Windows x64/);
  assert.match(notes, /Windows ARM64/);
  assert.match(notes, /macOS Apple Silicon/);
  assert.match(notes, /macOS Intel/);
  assert.match(notes, /English change/);
  assert.match(notes, /中文变更/);
  assert.match(
    notes,
    /\[guglerag-v1\.2\.0-linux-x86_64\.tar\.gz\]\(https:\/\/github\.com\/guglerag\/guglerag\/releases\/download\/v1\.2\.0\/guglerag-v1\.2\.0-linux-x86_64\.tar\.gz\)/
  );
  assert.match(
    notes,
    /\[guglerag-v1\.2\.0-linux-x86_64\.tar\.gz\.sha256\]\(https:\/\/github\.com\/guglerag\/guglerag\/releases\/download\/v1\.2\.0\/guglerag-v1\.2\.0-linux-x86_64\.tar\.gz\.sha256\)/
  );
});
