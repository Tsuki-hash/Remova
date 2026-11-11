import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const script = fileURLToPath(new URL("../check-internal-jargon.mjs", import.meta.url));
function fixture(run) {
  const root = mkdtempSync(join(tmpdir(), "remova-jargon-"));
  for (const dir of ["src", "scripts", ".github", "benchmarks", join("src-tauri", "src")]) {
    mkdirSync(join(root, dir), { recursive: true });
  }
  try { run(root, join(root, "src", "sample.ts")); }
  finally { rmSync(root, { recursive: true, force: true }); }
}
const check = root => spawnSync(process.execPath, [script, "--check"], { cwd: root, encoding: "utf8" });

test("rejects development identifier formats without changing source bytes", () => {
  fixture((root, file) => {
    for (const id of ["R987-98", "R-R987-98", "FE-R987-98", "R987-SEC-98", "R987-IO-98/BE-98", "REV-SEC-98", "F-98", "F-8", "CQ-98", "UX-98", "PF-98", "TC-98", "CD-98", "FC-98", "M-98", "R987 P2-98", "round-987", "round 987", "-98:", "A-98", "P-98"]) {
      const source = `// ${id}: protect the operation\nexport const safe = true;\n`;
      writeFileSync(file, source);
      const result = check(root);
      assert.equal(result.status, 1, id);
      assert.match(result.stderr, /sample.ts:1:/);
      assert.equal(readFileSync(file, "utf8"), source);
    }
  });
});

test("accepts explanatory comments and ordinary test string literals", () => {
  fixture((root, file) => {
    const source = '// Refuse untrusted ancestors.\nexport const sample = "R-R987-98";\n';
    writeFileSync(file, source);
    const result = check(root);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /0 hits/);
    assert.equal(readFileSync(file, "utf8"), source);
  });
});

test("rejects review tags in release-facing Markdown", () => {
  fixture((root) => {
    const file = join(root, "CHANGELOG.md");
    writeFileSync(file, "## 1.4.0\n\n- Fixed the round 987 regression.\n");
    const result = check(root);
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /CHANGELOG\.md:3:/);
  });
});

test("rejects filenames and script references while ignoring local measurements", () => {
  fixture((root, file) => {
    const name = "Q-97-startup.json";
    const publicFile = join(root, "benchmarks", name.replace(/\.json$/, ".md"));
    writeFileSync(publicFile, "# Benchmark\n");
    writeFileSync(file, `export const file = "${name}";\n`);
    const blocked = check(root);
    assert.equal(blocked.status, 1, blocked.stderr);
    assert.match(blocked.stderr, /public file name/);
    assert.match(blocked.stderr, /sample\.ts:1:/);
    rmSync(publicFile);
    writeFileSync(file, "export function p09_component_fixture() {}\n");
    assert.match(check(root).stderr, /sample\.ts:1:/);
    writeFileSync(file, 'export const file = "startup-cold.json";\n');
    mkdirSync(join(root, "benchmarks", "runs"));
    writeFileSync(join(root, "benchmarks", "runs", name), "{}");
    assert.equal(check(root).status, 0);
  });
});

test("fails when a declared scan root is missing", () => {
  const root = mkdtempSync(join(tmpdir(), "remova-jargon-"));
  mkdirSync(join(root, "src"));
  try {
    const result = check(root);
    assert.equal(result.status, 1);
    assert.match(result.stderr, /missing scan root/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
