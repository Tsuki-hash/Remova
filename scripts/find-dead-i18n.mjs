// Audit helper (REV-FE-14): list i18n keys defined in zh.ts but never
// referenced outside src/i18n, optionally prune them from zh.ts + en.ts.
//
//   node scripts/find-dead-i18n.mjs          # report only
//   node scripts/find-dead-i18n.mjs --check  # read-only gate, exit 1 on dead keys
//   node scripts/find-dead-i18n.mjs --prune  # remove dead keys from both files
import fs from "node:fs";
import path from "node:path";
import { dictionaryEntries } from "./i18n-dictionary.mjs";

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
const zhEntries = dictionaryEntries(zh, zhPath);
const keys = zhEntries.map(entry => entry.name);
// zh/en key-set parity — the compile-time check in i18n/index.ts also guards
// this, but the gate stays independent of the compiler invocation.
const en = fs.readFileSync(enPath, "utf8");
const enEntries = dictionaryEntries(en, enPath);
const enKeys = enEntries.map(entry => entry.name);
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
  const deadSet = new Set(dead);
  const candidates = [[zhPath, zh, zhEntries], [enPath, en, enEntries]].map(([file, original, entries]) => {
    let next = original;
    for (const entry of [...entries].reverse()) {
      if (deadSet.has(entry.name)) next = next.slice(0, entry.start) + next.slice(entry.end);
    }
    const remaining = dictionaryEntries(next, file).map(entry => entry.name);
    if (remaining.join("\0") !== entries.filter(entry => !deadSet.has(entry.name)).map(entry => entry.name).join("\0"))
      throw new Error(`${file}: prune changed retained keys`);
    return { file, original, next };
  });
  const staged = [], published = [];
  try {
    for (const candidate of candidates) {
      const temp = `${candidate.file}.prune-${process.pid}`;
      const fd = fs.openSync(temp, "wx");
      staged.push(temp);
      try { fs.writeFileSync(fd, candidate.next); } finally { fs.closeSync(fd); }
    }
    if (candidates.some(candidate => fs.readFileSync(candidate.file, "utf8") !== candidate.original))
      throw new Error("dictionary changed while preparing prune");
    for (const [index, candidate] of candidates.entries()) {
      fs.renameSync(staged[index], candidate.file);
      published.push(candidate);
    }
    for (const candidate of candidates) console.log(`${candidate.file}: removed ${dead.length} keys`);
  } catch (error) {
    for (const candidate of published) fs.writeFileSync(candidate.file, candidate.original);
    console.error(error.message);
    process.exitCode = 1;
  } finally {
    for (const temp of staged) fs.rmSync(temp, { force: true });
  }
}
