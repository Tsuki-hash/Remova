import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const script = fileURLToPath(new URL("../find-dead-i18n.mjs", import.meta.url));

function fixture(t, unused = true) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "remova-i18n-check-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.mkdirSync(path.join(root, "src/i18n"), { recursive: true });
  const dict = 'export const dict = {\n    used: "used",\n    unused: "unused",\n};\n';
  for (const lang of ["zh", "en"]) fs.writeFileSync(path.join(root, `src/i18n/${lang}.ts`), dict);
  fs.writeFileSync(path.join(root, "src/App.tsx"), unused ? "L.used" : "L.used; L.unused");
  return {
    run: (...args) => spawnSync(process.execPath, [script, ...args], { cwd: root, encoding: "utf8" }),
    unchanged: () => {
      for (const lang of ["zh", "en"]) {
        assert.equal(fs.readFileSync(path.join(root, `src/i18n/${lang}.ts`), "utf8"), dict);
      }
    },
  };
}

test("check fails on dead keys and leaves both dictionaries byte-for-byte unchanged", (t) => {
  const f = fixture(t);
  const result = f.run("--check");
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stdout, /dead: 1\nunused/);
  f.unchanged();
});

test("check succeeds when all keys are referenced and remains read-only", (t) => {
  const f = fixture(t, false);
  const result = f.run("--check");
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /dead: 0/);
  f.unchanged();
});

test("report mode preserves its successful, read-only behavior with dead keys", (t) => {
  const f = fixture(t);
  assert.equal(f.run().status, 0);
  f.unchanged();
});

test("conflicting or unknown flags fail before any write", (t) => {
  const f = fixture(t);
  for (const args of [["--check", "--prune"], ["--prune", "--unknown"], ["--chek"]]) {
    assert.equal(f.run(...args).status, 2);
    f.unchanged();
  }
});
