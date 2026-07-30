import fs from "node:fs";
import path from "node:path";
import { assertBinaryTarget } from "./binary-architecture.mjs";

function argument(name) {
  const index = process.argv.indexOf(name);
  if (index < 0 || !process.argv[index + 1]) throw new Error(`missing ${name}`);
  return process.argv[index + 1];
}

const platform = argument("--platform");
const arch = argument("--arch");
const binary = path.resolve(argument("--binary"));
const detected = assertBinaryTarget(fs.readFileSync(binary), platform, arch);

if (process.argv.includes("--native")) {
  const hostArch = { x64: "x86_64", arm64: "aarch64" }[process.arch];
  if (hostArch !== arch) {
    throw new Error(`expected native ${arch} runner, Node is running on ${process.arch}`);
  }
}

console.log(`validated ${detected.format} ${detected.arch} binary: ${binary}`);
