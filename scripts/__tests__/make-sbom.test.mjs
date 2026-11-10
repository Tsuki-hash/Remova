import assert from "node:assert/strict";
import { test } from "node:test";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { buildSbom, parseCargoLock, parsePackageLock, reachableCargoNames } from "../make-sbom.mjs";

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
  const input = {
    productName: "Remova",
    productVersion: "1.4.0",
    cargo: [
      { name: "remova", version: "1.4.0" },
      { name: "zip", version: "4.6.1" },
    ],
    npm: [
      { name: "react", version: "19.2.0" },
      { name: "zip", version: "4.6.1" },
    ],
  };
  const a = buildSbom(input);
  const b = buildSbom(input);
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

test("cargo reachability follows the resolved graph, not the whole lockfile", () => {
  // Real `cargo metadata` shape: resolved edges live in `resolve.nodes`, each
  // naming an exact package id, and a normal dependency has `kind: null`.
  const pkg = (id, name, version) => ({ id, name, version });
  const metadata = {
    packages: [
      pkg("remova 1.4.1", "remova", "1.4.1"),
      pkg("tauri 2.0.0", "tauri", "2.0.0"),
      pkg("zip 4.6.1", "zip", "4.6.1"),
      pkg("base64 0.22.1", "base64", "0.22.1"),
      pkg("base64 0.21.7", "base64", "0.21.7"),
      pkg("trybuild 1.0.0", "trybuild", "1.0.0"),
      pkg("lockfile-only 1.0.0", "lockfile-only-crate", "1.0.0"),
    ],
    workspace_members: ["remova 1.4.1"],
    resolve: {
      nodes: [
        {
          id: "remova 1.4.1",
          deps: [
            { pkg: "tauri 2.0.0", dep_kinds: [{ kind: null }] },
            // A dev-only edge must not make the crate ship.
            { pkg: "trybuild 1.0.0", dep_kinds: [{ kind: "dev" }] },
          ],
        },
        {
          id: "tauri 2.0.0",
          deps: [
            { pkg: "zip 4.6.1", dep_kinds: [{ kind: null }] },
            { pkg: "base64 0.22.1", dep_kinds: [{ kind: null }] },
          ],
        },
        { id: "zip 4.6.1", deps: [] },
        { id: "base64 0.22.1", deps: [] },
        { id: "base64 0.21.7", deps: [] },
        { id: "trybuild 1.0.0", deps: [] },
        { id: "lockfile-only 1.0.0", deps: [] },
      ],
    },
  };
  const seen = reachableCargoNames(metadata);
  assert.ok(seen.has("remova@1.4.1"));
  assert.ok(seen.has("tauri@2.0.0"));
  assert.ok(seen.has("zip@4.6.1"));
  assert.ok(seen.has("base64@0.22.1"));
  assert.ok(!seen.has("base64@0.21.7"), "an unreachable version of a reachable crate stays out");
  assert.ok(!seen.has("trybuild@1.0.0"), "dev dependencies do not ship");
  assert.ok(!seen.has("lockfile-only-crate@1.0.0"), "unreachable entries do not ship");
});
