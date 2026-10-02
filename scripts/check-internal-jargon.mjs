#!/usr/bin/env node
// Fail when tracked sources still embed internal review/priority tags.
// Usage: node scripts/check-internal-jargon.mjs --check
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const roots = ["src", "src-tauri/src", "src-tauri/tests"];
const idRe =
  /(?:^|[^A-Za-z0-9])(?:REV-[A-Z]+-\d+|R\d+-(?:[A-Z]+-)?\d+(?:\/[A-Z]+-\d+)?|R-R\d+-\d+|(?:FE|BE|SEC|SUP|UX|QA)-R\d+-\d+|FN-\d+|CODE-\d+|ARCH-\d+|AR-\d+|SEC-\d+|PERF-\d+|FUNC-\d+|NEW-[A-Z]|FE-P\d+[a-z]?|P0-\d|Q-[A-Z]\d+|S-R\d+-\d+|S-\d+|T-R\d+|F-R\d+|M\d+\+\d+|P[0-3]\b)/;

function walk(dir, out) {
  let entries;
  try {
    entries = readdirSync(dir);
  } catch {
    return;
  }
  for (const name of entries) {
    const p = join(dir, name);
    const st = statSync(p);
    if (st.isDirectory()) walk(p, out);
    else if (/\.(rs|ts|tsx|mjs|js)$/.test(name)) out.push(p);
  }
}

const files = [];
for (const r of roots) walk(r, files);
const hits = [];
for (const f of files) {
  const text = readFileSync(f, "utf8");
  text.split(/\r?\n/).forEach((line, i) => {
    // Only flag comment-ish lines to avoid string literals in tests.
    if (!/^\s*(\/\/|\/\*|\*|#|<!--)/.test(line) && !/\/\//.test(line) && !/\/\*/.test(line)) {
      return;
    }
    if (idRe.test(line)) hits.push(`${relative(process.cwd(), f)}:${i + 1}:${line.trim()}`);
  });
}

if (hits.length) {
  console.error(`internal jargon hits: ${hits.length}`);
  for (const h of hits.slice(0, 40)) console.error(h);
  if (hits.length > 40) console.error(`... +${hits.length - 40} more`);
  process.exit(1);
}
console.log("internal jargon: 0 hits");
