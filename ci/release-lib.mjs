import fs from "node:fs";
import path from "node:path";

export const releaseTargets = [
  { platform: "linux", arch: "x86_64", format: "tar.gz", label: "Linux x64" },
  { platform: "windows", arch: "x86_64", format: "zip", label: "Windows x64" },
  { platform: "macos", arch: "aarch64", format: "tar.gz", label: "macOS Apple Silicon" }
];

export function extractChangelogSection(source, version) {
  const escaped = version.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const heading = new RegExp(`^## \\[${escaped}\\][^\\n]*$`, "m");
  const match = heading.exec(source);
  if (!match) throw new Error(`missing changelog section: ## [${version}]`);
  const start = match.index + match[0].length;
  const remainder = source.slice(start);
  const nextHeading = remainder.search(/^## \[/m);
  return remainder.slice(0, nextHeading >= 0 ? nextHeading : undefined).trim();
}

export function classifyArtifacts(fileNames, version) {
  const rows = [];
  const expected = new Set();
  for (const target of releaseTargets) {
    const archive = `guglerag-v${version}-${target.platform}-${target.arch}.${target.format}`;
    expected.add(archive);
    expected.add(`${archive}.sha256`);
    rows.push({ ...target, archive, checksum: `${archive}.sha256` });
  }

  const actual = new Set(fileNames);
  const unknown = [...actual].filter((name) => !expected.has(name));
  const missing = [...expected].filter((name) => !actual.has(name));
  if (unknown.length) throw new Error(`unclassified release artifacts: ${unknown.join(", ")}`);
  if (missing.length) throw new Error(`missing release artifacts: ${missing.join(", ")}`);
  return rows;
}

export function renderReleaseNotes({ version, english, chinese, files }) {
  const rows = classifyArtifacts(files, version);
  const table = rows
    .map((row) => `| ${row.label} | \`${row.archive}\` | \`${row.checksum}\` |`)
    .join("\n");
  return `# GugleRAG v${version}

## English

${english}

## 简体中文

${chinese}

## Downloads / 下载

| Platform | Portable archive | SHA-256 |
| --- | --- | --- |
${table}

All archives are unsigned portable builds. Extract the archive, edit \`.env\` or use the setup wizard, then run the executable from the extracted directory so it can serve \`frontend/dist\`.

所有压缩包均为未签名便携构建。解压后编辑 \`.env\` 或使用初始化向导，并从解压目录运行可执行文件，以便服务端读取 \`frontend/dist\`。
`;
}

export function artifactFiles(directory) {
  return fs
    .readdirSync(directory, { withFileTypes: true })
    .filter((entry) => entry.isFile())
    .map((entry) => entry.name)
    .sort();
}

export function readChangelog(file, version) {
  return extractChangelogSection(fs.readFileSync(path.resolve(file), "utf8"), version);
}
