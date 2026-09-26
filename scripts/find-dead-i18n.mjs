// Audit helper (REV-FE-14): list i18n keys defined in zh.ts but never
// referenced outside src/i18n, optionally prune them from zh.ts + en.ts.
//
//   node scripts/find-dead-i18n.mjs          # report only
//   node scripts/find-dead-i18n.mjs --prune  # remove dead keys from both files
import fs from "node:fs";
import path from "node:path";

const zhPath = "src/i18n/zh.ts";
const enPath = "src/i18n/en.ts";
const prune = process.argv.includes("--prune");

const zh = fs.readFileSync(zhPath, "utf8");
const keys = [...zh.matchAll(/^    ([A-Za-z0-9_]+):/gm)].map((m) => m[1]);

function walk(dir, out = []) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) {
      if (["node_modules", "dist", ".git"].includes(e.name)) continue;
      walk(p, out);
    } else if (/\.(ts|tsx)$/.test(e.name)) out.push(p);
  }
  return out;
}

const files = walk("src").filter((f) => !f.replace(/\\/g, "/").startsWith("src/i18n/"));
const used = new Set();
for (const f of files) {
  const t = fs.readFileSync(f, "utf8");
  for (const k of keys) if (t.includes(k)) used.add(k);
}
const dead = keys.filter((k) => !used.has(k));
console.log("total keys:", keys.length, "dead:", dead.length);

if (!prune) {
  console.log(dead.join("\n"));
} else {
  // Entry spans from `    key:` until the first line ending with `,` (values are
  // single strings, multi-line strings or `=>` + template — all terminate that way).
  const deadSet = new Set(dead);
  for (const file of [zhPath, enPath]) {
    const lines = fs.readFileSync(file, "utf8").split("\n");
    const out = [];
    let skipping = false;
    let removed = 0;
    for (const line of lines) {
      if (skipping) {
        if (/,\s*$/.test(line)) skipping = false;
        else continue;
        continue;
      }
      const m = line.match(/^    ([A-Za-z0-9_]+):/);
      if (m && deadSet.has(m[1])) {
        removed++;
        if (!/,\s*$/.test(line)) skipping = true;
        continue;
      }
      out.push(line);
    }
    fs.writeFileSync(file, out.join("\n"));
    console.log(`${file}: removed ${removed} keys`);
  }
}
