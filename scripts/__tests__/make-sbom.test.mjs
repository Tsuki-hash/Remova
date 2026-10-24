import assert from "node:assert/strict";
import { test } from "node:test";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { buildSbom, parseCargoLock, parsePackageLock } from "../make-sbom.mjs";

test("parses cargo lock packages", () => {
  const cargo = ['[package]', 'name = "remova"', 'version = "1.4.0"', "", "[[package]]", 'name = "serde"', 'version = "1.0.219"'].join("\n");
  assert.deepEqual(parseCargoLock(cargo), [
    { name: "remova", version: "1.4.0" },
    { name: "serde", version: "1.0.219" },
  ]);
});

test("package lock keeps production dependencies only", () => {
  const lock = {
    packages: {
      "": { name: "remova-next", version: "1.4.0" },
      "node_modules/react": { version: "19.2.0" },
      "node_modules/vitest": { version: "5.0.5", dev: true },
      "node_modules/esbuild": { version: "0.25.12", devOptional: true },
      "node_modules/react/node_modules/scheduler": { version: "0.27.0" },
    },
  };
  assert.deepEqual(parsePackageLock(lock), [
    { name: "react", version: "19.2.0" },
    { name: "scheduler", version: "0.27.0" },
  ]);
});

test("sbom is deterministic and drops the application crate", () => {
  const components = [
    { productName: "Remova", productVersion: "1.4.0" },
    {
      cargo: [
        { name: "remova", version: "1.4.0" },
        { name: "zip", version: "4.6.1" },
      ],
      npm: [
        { name: "react", version: "19.2.0" },
        { name: "zip", version: "4.6.1" },
      ],
    },
  ];
  const a = buildSbom(...components);
  const b = buildSbom(...components);
  assert.deepEqual(a, b);
  assert.equal(a.specVersion, "1.5");
  assert.deepEqual(
    a.components.map((c) => c.name),
    ["react", "zip"],
  );
  assert.match(a.serialNumber, /^urn:uuid:[0-9a-f-]{36}$/);
});

test("generates an sbom from the repository lockfiles", () => {
  const dir = mkdtempSync(path.join(tmpdir(), "sbom-"));
  try {
    const out = path.join(dir, "SBOM.cdx.json");
    execFileSync(process.execPath, [path.resolve(import.meta.dirname, "..", "make-sbom.mjs"), "--out", out], {
      cwd: path.resolve(import.meta.dirname, "..", ".."),
    });
    const sbom = JSON.parse(readFileSync(out, "utf8"));
    assert.equal(sbom.metadata.component.name, "Remova");
    assert.ok(sbom.components.length > 100, `expected the full dependency set, got ${sbom.components.length}`);
    const names = new Set(sbom.components.map((c) => c.name));
    assert.ok(names.has("tauri"));
    assert.ok(names.has("react"));
    assert.ok(!names.has("vitest"), "dev tooling must not appear in the sbom");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
