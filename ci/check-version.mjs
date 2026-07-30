import fs from "node:fs";

function argument(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

function requireMatch(source, pattern, label) {
  const match = source.match(pattern);
  if (!match) throw new Error(`unable to read ${label}`);
  return match[1];
}

const cargoToml = fs.readFileSync("Cargo.toml", "utf8");
const cargoLock = fs.readFileSync("Cargo.lock", "utf8");
const frontend = JSON.parse(fs.readFileSync("frontend/package.json", "utf8"));
const frontendLock = JSON.parse(fs.readFileSync("frontend/package-lock.json", "utf8"));
const version = requireMatch(
  cargoToml,
  /\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m,
  "Cargo.toml package version"
);
const lockVersion = requireMatch(
  cargoLock,
  /\[\[package\]\]\s*\r?\nname = "GugleRAG"\s*\r?\nversion = "([^"]+)"/,
  "Cargo.lock GugleRAG version"
);
const versions = new Map([
  ["Cargo.toml", version],
  ["Cargo.lock", lockVersion],
  ["frontend/package.json", frontend.version],
  ["frontend/package-lock.json", frontendLock.version],
  ["frontend/package-lock.json root package", frontendLock.packages?.[""]?.version]
]);
for (const [file, actual] of versions) {
  if (actual !== version) throw new Error(`${file} version ${actual} does not match ${version}`);
}

for (const file of ["CHANGELOG.md", "CHANGELOG.zh-CN.md"]) {
  const source = fs.readFileSync(file, "utf8");
  if (!source.includes(`## [${version}]`)) {
    throw new Error(`${file} is missing ## [${version}]`);
  }
}

const tag = argument("--tag");
if (tag && tag !== `v${version}`) {
  throw new Error(`release tag ${tag} does not match manifest version v${version}`);
}

console.log(`validated synchronized version ${version}`);
