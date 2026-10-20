import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const script = fileURLToPath(new URL("../check-reviews-index.mjs", import.meta.url));

function fixture(t, indexBody) {
  const root = mkdtempSync(join(tmpdir(), "remova-reviews-index-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, "docs", "reviews", "2026-01-01"), { recursive: true });
  writeFileSync(join(root, "docs", "reviews", "README.md"), indexBody);
  writeFileSync(join(root, "docs", "reviews", "2026-01-01", "2026-01-01-第01轮-样例.md"), "# x\n");
  return root;
}

const run = (root) =>
  spawnSync(process.execPath, [script, "--root", root], { encoding: "utf8" });

test("skips only when the whole docs tree is absent", (t) => {
  const root = mkdtempSync(join(tmpdir(), "remova-reviews-nodocs-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const result = run(root);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /SKIP/);
});

test("fails when a report is missing from the index", (t) => {
  const root = fixture(t, "# index\n");
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /2026-01-01-第01轮-样例\.md/);
});

test("passes when every report is named", (t) => {
  const root = fixture(t, "# index\n[样例](./2026-01-01/2026-01-01-第01轮-样例.md)\n");
  const result = run(root);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /OK: all 1 file/);
});
