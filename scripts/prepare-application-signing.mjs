import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const [application, output] = process.argv.slice(2);
if (!application || !output) {
  throw new Error("Usage: node prepare-application-signing.mjs <remova.exe> <output>");
}

const bytes = readFileSync(application);
if (bytes.length < 64 || bytes.readUInt16LE(0) !== 0x5a4d) {
  throw new Error("Expected a Windows executable.");
}
const pe = bytes.readUInt32LE(0x3c);
if (pe + 24 > bytes.length || bytes.readUInt32LE(pe) !== 0x4550) {
  throw new Error("Invalid PE signature.");
}
const count = bytes.readUInt16LE(pe + 6);
const firstSection = pe + 24 + bytes.readUInt16LE(pe + 20);
if (firstSection + count * 40 > bytes.length) {
  throw new Error("Truncated PE section table.");
}

// Tauri uses this embedded marker to select the updater channel. Its bundler
// rewrites UNK for MSI/NSIS, so give each signer the bytes that will ship.
const marker = Buffer.from("__TAURI_BUNDLE_TYPE_VAR_UNK");
const offset = bytes.indexOf(marker);
if (offset < 0 || bytes.indexOf(marker, offset + 1) !== -1) {
  throw new Error("Expected exactly one unpatched Tauri bundle marker.");
}
let inReadOnlyData = false;
for (let i = 0; i < count; i++) {
  const section = firstSection + i * 40;
  if (bytes.toString("ascii", section, section + 8) !== ".rdata\0\0") continue;
  const start = bytes.readUInt32LE(section + 20);
  const end = start + bytes.readUInt32LE(section + 16);
  inReadOnlyData = end <= bytes.length && offset >= start && offset + marker.length <= end;
}
if (!inReadOnlyData) throw new Error("The bundle marker is outside PE read-only data.");

for (const [channel, type] of [["portable", "UNK"], ["msi", "MSI"], ["nsis", "NSS"]]) {
  const variant = Buffer.from(bytes);
  variant.write(type, offset + marker.length - type.length, "ascii");
  const dir = join(output, channel);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "remova.exe"), variant, { flag: "wx" });
}
