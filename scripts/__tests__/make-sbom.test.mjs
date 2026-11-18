import assert from "node:assert/strict";
import { test } from "node:test";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { buildSbom, parsePackageLock } from "../make-sbom.mjs";

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
    const generatedNotices = path.join(dir, "THIRD-PARTY-NOTICES.md");
    execFileSync(process.execPath, [path.resolve(import.meta.dirname, "..", "make-sbom.mjs"), "--out", out, "--notices", generatedNotices, "--check-notices"], {
      cwd: path.resolve(import.meta.dirname, "..", ".."),
    });
    const sbom = JSON.parse(readFileSync(out, "utf8"));
    assert.equal(sbom.metadata.component.name, "Remova");
    assert.ok(sbom.components.length > 100, `expected the full dependency set, got ${sbom.components.length}`);
    const names = new Set(sbom.components.map((c) => c.name));
    assert.ok(names.has("tauri"));
    assert.ok(names.has("react"));
    assert.ok(!names.has("vitest"), "dev tooling must not appear in the sbom");
    for (const name of ["linux-raw-sys", "wasi", "windows_aarch64_msvc", "windows_i686_gnu"]) {
      assert.ok(!names.has(name), `${name} is outside the Windows x64 release graph`);
    }
    for (const name of ["chrono", "defmt", "jiff"]) {
      assert.ok(!names.has(name), `${name} is outside the normal Windows graph`);
    }
    const tree = execFileSync("cargo", [
      "tree", "--locked", "--target", "x86_64-pc-windows-msvc",
      "--features", "tauri/custom-protocol", "-e", "normal",
      "--prefix", "depth", "--format", " {p}",
    ], { cwd: path.resolve(import.meta.dirname, "../../src-tauri"), encoding: "utf8" });
    const expected = new Set(tree.trim().split(/\r?\n/).map(line => {
      const [, name, version] = line.split(" ");
      return `${name}@${version.slice(1)}`;
    }));
    expected.delete(`remova@${sbom.metadata.component.version}`);
    const actual = new Set(sbom.components.filter(c => c.purl.startsWith("pkg:cargo/")).map(c => `${c.name}@${c.version}`));
    assert.deepEqual(actual, expected);
    assert.ok([...actual].filter(key => key.startsWith("windows-sys@")).length > 1);
    const notices = readFileSync(generatedNotices, "utf8");
    const purls = [...notices.matchAll(/\((pkg:[^)]+)\)/g)].map((match) => match[1]);
    assert.deepEqual(purls, sbom.components.map((c) => c.purl));
    const titles = new Set([...notices.matchAll(/^### Text ([a-f0-9]{64})$/gm)].map((match) => match[1]));
    for (const [, digest] of notices.matchAll(/\]\(#text-([a-f0-9]{64})\)/g)) assert.ok(titles.has(digest));
    assert.match(notices, /^- ring 0\.17\.14 .*Apache-2\.0 AND ISC.*`LICENSE-other-bits`.*`third_party\/fiat\/LICENSE`/m);
    const componentLines = notices.split("\n");
    const loaderNotice = componentLines.find((line) => line.startsWith("- webview2-com-sys "));
    assert.match(loaderNotice ?? "", /^- webview2-com-sys 0\.39\.1 .*`Microsoft\.Web\.WebView2 1\.0\.3800\.47 NOTICE\.txt`/);
    const selectorsNotice = componentLines.find((line) => line.startsWith("- selectors "));
    assert.match(selectorsNotice ?? "", /^- selectors 0\.38\.0 .*MPL-2\.0.*\[Corresponding source\]/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
