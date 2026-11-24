import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const script = fileURLToPath(new URL("../prepare-application-signing.mjs", import.meta.url));
const marker = "__TAURI_BUNDLE_TYPE_VAR_UNK";
const offset = 256;

function fixture() {
  const bytes = Buffer.alloc(512);
  bytes.writeUInt16LE(0x5a4d, 0);
  bytes.writeUInt32LE(64, 0x3c);
  bytes.writeUInt32LE(0x4550, 64);
  bytes.writeUInt16LE(1, 70);
  bytes.write(".rdata", 88, "ascii");
  bytes.writeUInt32LE(128, 104);
  bytes.writeUInt32LE(offset, 108);
  bytes.write(marker, offset, "ascii");
  return bytes;
}

function run(bytes, verify) {
  const dir = mkdtempSync(join(tmpdir(), "remova-signing-variant-test-"));
  try {
    const input = join(dir, "input.exe");
    const output = join(dir, "variants");
    writeFileSync(input, bytes);
    const result = spawnSync(process.execPath, [script, input, output], { encoding: "utf8" });
    verify(result, output);
    assert.deepEqual(readFileSync(input), bytes);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("signing variants preserve the original image except for the expected channel marker", () => {
  const bytes = fixture();
  run(bytes, (result, output) => {
    assert.equal(result.status, 0, result.stderr);
    for (const [channel, type] of [["portable", "UNK"], ["msi", "MSI"], ["nsis", "NSS"]]) {
      const expected = Buffer.from(bytes);
      expected.write(type, offset + marker.length - 3, "ascii");
      assert.deepEqual(readFileSync(join(output, channel, "remova.exe")), expected);
    }
  });
});

test("signing refuses duplicate markers and markers outside read-only data", () => {
  const duplicate = fixture();
  duplicate.write(marker, offset + marker.length, "ascii");
  run(duplicate, result => {
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /exactly one/);
  });
  const outOfSection = fixture();
  outOfSection.writeUInt32LE(offset + 64, 108);
  run(outOfSection, result => {
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /outside PE read-only data/);
  });
});
