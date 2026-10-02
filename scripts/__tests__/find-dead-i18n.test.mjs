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
    root,
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

test("parity failures refuse check and prune without modifying dictionaries", (t) => {
  const f = fixture(t, false);
  const en = path.join(f.root, "src/i18n/en.ts");
  fs.writeFileSync(en, 'export const dict = {\n    used: "used",\n    extra: "extra",\n};\n');
  const paths = [en, path.join(f.root, "src/i18n/zh.ts")];
  const before = paths.map(p => fs.readFileSync(p, "utf8"));
  for (const mode of ["--check", "--prune"]) {
    const result = f.run(mode);
    assert.equal(result.status, 1);
    assert.match(result.stderr, /i18n key parity broken/);
    assert.deepEqual(paths.map(p => fs.readFileSync(p, "utf8")), before);
  }
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

test("prune handles multiline templates, comments and regex without cutting retained entries", (t) => {
  const f = fixture(t);
  const originals = [
    'export const dict = {\n    used: "(",\n    unused: (n: number) => `line (\n${n},\n`,\n};\n',
    'export const dict = {\n    used: "}",\n    unused: (n: number) => { /* ( { */ return /[(){}]/.test(String(n)) ? `,${n}` : "{"; },\n};\n',
  ];
  const paths = ["zh", "en"].map(lang => path.join(f.root, `src/i18n/${lang}.ts`));
  paths.forEach((file, index) => fs.writeFileSync(file, originals[index]));
  const result = f.run("--prune");
  assert.equal(result.status, 0, result.stderr);
  for (const [index, file] of paths.entries()) {
    const next = fs.readFileSync(file, "utf8");
    assert.ok(!next.includes("unused:"));
    assert.ok(next.includes(index === 0 ? 'used: "("' : 'used: "}"'));
  }
  assert.equal(f.run("--check").status, 0);
});

test("a malformed second dictionary refuses prune without publishing the first", (t) => {
  const f = fixture(t);
  const paths = ["zh", "en"].map(lang => path.join(f.root, `src/i18n/${lang}.ts`));
  fs.writeFileSync(paths[1], 'export const dict = {\n    used: "used",\n    unused: (n: number) => ,\n};\n');
  const before = paths.map(file => fs.readFileSync(file, "utf8"));
  assert.notEqual(f.run("--prune").status, 0);
  assert.deepEqual(paths.map(file => fs.readFileSync(file, "utf8")), before);
});

test("a second publish failure restores both dictionaries and removes owned staging files", (t) => {
  const f = fixture(t);
  const preload = path.join(f.root, "publish-failure.cjs");
  fs.writeFileSync(preload, "const fs = require('node:fs'); const rename = fs.renameSync; fs.renameSync = (from, to) => { if (to.endsWith('en.ts')) throw new Error('denied'); return rename(from, to); };");
  const result = spawnSync(process.execPath, ["--require", preload, script, "--prune"], { cwd: f.root, encoding: "utf8" });
  assert.equal(result.status, 1, result.stderr);
  f.unchanged();
  assert.deepEqual(fs.readdirSync(path.join(f.root, "src/i18n")).sort(), ["en.ts", "zh.ts"]);
});
