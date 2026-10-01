// Audit helper (REV-FE-14): list i18n keys defined in zh.ts but never
// referenced outside src/i18n, optionally prune them from zh.ts + en.ts.
//
//   node scripts/find-dead-i18n.mjs          # report only
//   node scripts/find-dead-i18n.mjs --check  # read-only gate, exit 1 on dead keys
//   node scripts/find-dead-i18n.mjs --prune  # remove dead keys from both files
import fs from "node:fs";
import path from "node:path";

const zhPath = "src/i18n/zh.ts";
const enPath = "src/i18n/en.ts";
const args = process.argv.slice(2);
if (args.some((arg) => !["--check", "--prune"].includes(arg)) ||
    (args.includes("--check") && args.includes("--prune"))) {
  console.error("Usage: node scripts/find-dead-i18n.mjs [--check | --prune]");
  process.exit(2);
}
const prune = args.includes("--prune");
const check = args.includes("--check");

const zh = fs.readFileSync(zhPath, "utf8");
const keys = [...zh.matchAll(/^    ([A-Za-z0-9_]+):/gm)].map((m) => m[1]);
// zh/en key-set parity — the compile-time check in i18n/index.ts also guards
// this, but the gate stays independent of the compiler invocation.
const en = fs.readFileSync(enPath, "utf8");
const enKeys = [...en.matchAll(/^    ([A-Za-z0-9_]+):/gm)].map((m) => m[1]);
const onlyZh = keys.filter((k) => !enKeys.includes(k));
const onlyEn = enKeys.filter((k) => !keys.includes(k));
if (onlyZh.length || onlyEn.length) {
  const msg = `i18n key parity broken — zh only: ${onlyZh.join(", ") || "(none)"}; en only: ${onlyEn.join(", ") || "(none)"}`;
  if (check || prune) {
    console.error(msg);
    process.exit(1);
  }
  console.warn(msg);
}

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
  if (check && dead.length > 0) process.exitCode = 1;
} else {
  // R21-QA-07: depth-aware entry end (a value's internal line may also end in
  // a comma). An entry starts at `    key:` and ends at the first line that
  // terminates the value — depth back to 0 and a trailing comma.
  const deadSet = new Set(dead);
  for (const file of [zhPath, enPath]) {
    const original = fs.readFileSync(file, "utf8");
    const lines = original.split("\n");
    const out = [];
    let skipping = false;
    let depth = 0;
    let removed = 0;
    for (const line of lines) {
      if (skipping) {
        for (const ch of line) {
          if (ch === "(" || ch === "[" || ch === "{") depth++;
          else if (ch === ")" || ch === "]" || ch === "}") depth--;
        }
        if (depth <= 0 && /,\s*$/.test(line)) {
          skipping = false;
          depth = 0;
        }
        continue;
      }
      const m = line.match(/^    ([A-Za-z0-9_]+):/);
      if (m && deadSet.has(m[1])) {
        removed++;
        if (!/,\s*$/.test(line)) {
          skipping = true;
          depth = 0;
          for (const ch of line) {
            if (ch === "(" || ch === "[" || ch === "{") depth++;
            else if (ch === ")" || ch === "]" || ch === "}") depth--;
          }
        }
        continue;
      }
      out.push(line);
    }
    const next = out.join("\n");
    // R21-QA-07: write-back only if the pruned file still looks like a valid
    // object literal — a mid-entry cut must roll back, not ship broken TS.
    const opens = (next.match(/\{/g) || []).length;
    const closes = (next.match(/\}/g) || []).length;
    const parensOpen = (next.match(/\(/g) || []).length;
    const parensClose = (next.match(/\)/g) || []).length;
    if (opens !== closes || parensOpen !== parensClose) {
      console.error(`${file}: prune would unbalance braces/parens — rolled back`);
      continue;
    }
    fs.writeFileSync(file, next);
    console.log(`${file}: removed ${removed} keys`);
  }
}
