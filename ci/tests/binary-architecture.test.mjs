import assert from "node:assert/strict";
import test from "node:test";
import { assertBinaryTarget, inspectBinary } from "../binary-architecture.mjs";

function pe(machine) {
  const buffer = Buffer.alloc(128);
  buffer.write("MZ", 0, "ascii");
  buffer.writeUInt32LE(64, 0x3c);
  buffer.writeUInt32LE(0x00004550, 64);
  buffer.writeUInt16LE(machine, 68);
  return buffer;
}

function elf(machine) {
  const buffer = Buffer.alloc(64);
  buffer.set([0x7f, 0x45, 0x4c, 0x46, 2, 1]);
  buffer.writeUInt16LE(machine, 18);
  return buffer;
}

function machO(cpuType) {
  const buffer = Buffer.alloc(32);
  buffer.writeUInt32LE(0xfeedfacf, 0);
  buffer.writeUInt32LE(cpuType, 4);
  return buffer;
}

test("detects x86_64 and ARM64 PE binaries", () => {
  assert.deepEqual(inspectBinary(pe(0x8664)), { format: "pe", arch: "x86_64" });
  assert.deepEqual(inspectBinary(pe(0xaa64)), { format: "pe", arch: "aarch64" });
});

test("detects x86_64 and ARM64 ELF binaries", () => {
  assert.deepEqual(inspectBinary(elf(62)), { format: "elf", arch: "x86_64" });
  assert.deepEqual(inspectBinary(elf(183)), { format: "elf", arch: "aarch64" });
});

test("detects x86_64 and ARM64 Mach-O binaries", () => {
  assert.deepEqual(inspectBinary(machO(0x01000007)), {
    format: "mach-o",
    arch: "x86_64"
  });
  assert.deepEqual(inspectBinary(machO(0x0100000c)), {
    format: "mach-o",
    arch: "aarch64"
  });
});

test("rejects a binary for the wrong target", () => {
  assert.throws(() => assertBinaryTarget(pe(0x8664), "windows", "aarch64"));
  assert.throws(() => assertBinaryTarget(elf(62), "macos", "x86_64"));
});
