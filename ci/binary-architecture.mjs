const formatsByPlatform = {
  linux: "elf",
  windows: "pe",
  macos: "mach-o"
};

function requireLength(buffer, length, format) {
  if (buffer.length < length) throw new Error(`truncated ${format} binary`);
}

function mappedArchitecture(mapping, value, format) {
  const arch = mapping[value];
  if (!arch) throw new Error(`unsupported ${format} architecture: 0x${value.toString(16)}`);
  return arch;
}

export function inspectBinary(buffer) {
  requireLength(buffer, 4, "executable");

  if (buffer[0] === 0x4d && buffer[1] === 0x5a) {
    requireLength(buffer, 64, "PE");
    const header = buffer.readUInt32LE(0x3c);
    requireLength(buffer, header + 6, "PE");
    if (buffer.readUInt32LE(header) !== 0x00004550) throw new Error("invalid PE signature");
    const arch = mappedArchitecture(
      { 0x8664: "x86_64", 0xaa64: "aarch64" },
      buffer.readUInt16LE(header + 4),
      "PE"
    );
    return { format: "pe", arch };
  }

  if (buffer[0] === 0x7f && buffer.subarray(1, 4).toString("ascii") === "ELF") {
    requireLength(buffer, 20, "ELF");
    if (buffer[4] !== 2 || buffer[5] !== 1) {
      throw new Error("expected a little-endian 64-bit ELF binary");
    }
    const arch = mappedArchitecture(
      { 62: "x86_64", 183: "aarch64" },
      buffer.readUInt16LE(18),
      "ELF"
    );
    return { format: "elf", arch };
  }

  if (buffer.readUInt32LE(0) === 0xfeedfacf) {
    requireLength(buffer, 8, "Mach-O");
    const arch = mappedArchitecture(
      { 0x01000007: "x86_64", 0x0100000c: "aarch64" },
      buffer.readUInt32LE(4),
      "Mach-O"
    );
    return { format: "mach-o", arch };
  }

  throw new Error("unsupported executable format");
}

export function assertBinaryTarget(buffer, platform, arch) {
  const detected = inspectBinary(buffer);
  const expectedFormat = formatsByPlatform[platform];
  if (!expectedFormat) throw new Error(`unsupported target platform: ${platform}`);
  if (detected.format !== expectedFormat) {
    throw new Error(`expected ${expectedFormat} binary for ${platform}, found ${detected.format}`);
  }
  if (detected.arch !== arch) {
    throw new Error(`expected ${arch} binary, found ${detected.arch}`);
  }
  return detected;
}
