import fs from "node:fs";
import path from "node:path";

function argument(name) {
  const index = process.argv.indexOf(name);
  if (index < 0 || !process.argv[index + 1]) throw new Error(`missing ${name}`);
  return process.argv[index + 1];
}

const version = argument("--version");
const platform = argument("--platform");
const arch = argument("--arch");
const binary = path.resolve(argument("--binary"));
const frontend = path.resolve("frontend/dist");
for (const [label, requiredPath] of [["binary", binary], ["frontend build", frontend]]) {
  if (!fs.existsSync(requiredPath)) throw new Error(`missing ${label}: ${requiredPath}`);
}

const packageName = `GugleRAG-v${version}-${platform}-${arch}`;
const root = path.resolve("release/stage", packageName);
fs.rmSync(root, { recursive: true, force: true });
fs.mkdirSync(path.join(root, "frontend"), { recursive: true });
fs.copyFileSync(binary, path.join(root, path.basename(binary)));
if (platform !== "windows") fs.chmodSync(path.join(root, path.basename(binary)), 0o755);
fs.cpSync(frontend, path.join(root, "frontend/dist"), { recursive: true });
for (const file of [".env.example", "README.md", "CHANGELOG.md", "CHANGELOG.zh-CN.md"]) {
  fs.copyFileSync(file, path.join(root, file));
}
fs.writeFileSync(
  path.join(root, "RELEASE-METADATA.json"),
  `${JSON.stringify({ version, platform, arch, signed: false }, null, 2)}\n`
);
console.log(`assembled ${root}`);
