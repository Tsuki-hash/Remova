#!/usr/bin/env node
// Reject development identifiers in public source and documentation.
// Usage: node scripts/check-internal-jargon.mjs --check
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const roots = ["src", "src-tauri/src", "scripts", ".github", "benchmarks"];
const rootFiles = [
  "eslint.config.mjs",
  "vite.config.ts",
  "tsconfig.json",
  "index.html",
  "src-tauri/build.rs",
  "CHANGELOG.md",
  "README.md",
  "README.en.md",
  "RECOVERY.md",
  "CONFIGURATION.md",
  "CONTRIBUTING.md",
  "RELEASE-STATUS.md",
  "SECURITY.md",
];
// The gate's own regex/test fixtures intentionally contain these identifiers.
const excluded = new Set([
  "scripts/check-internal-jargon.mjs",
  "scripts/__tests__/check-internal-jargon.test.mjs",
]);
const idRe =
  /(?:^|[^A-Za-z0-9])(?:REV-[A-Z]+-\d+|R\d+-(?:[A-Z]+-)?\d+(?:\/[A-Z]+-\d+)?|R-R\d+-\d+|R\d+ P[0-3]-\d+|(?:FE|BE|SEC|SUP|UX|QA)-R\d+-\d+|FN-\d+|F-\d+|M-\d+|[Rr]ound[\s-]?\d+|CODE-\d+|ARCH-\d+|AR-\d+|SEC-\d+|PERF-\d+|FUNC-\d+|(?:CQ|UX|PF|TC|CD|FC)-\d+|NEW-[A-Z]|FE-P\d+[a-z]?|P0-\d|Q-[A-Z]\d+|S-R\d+-\d+|S-\d+|T-R\d+|F-R\d+|M\d+\+\d+|P[0-3]\b|[A-Z]-\d{1,2}(?=\s|:|,|\)|$)|\s-\d{1,2}(?=\s|:|$))/;

const artifactReference = /(?:^|[^a-z0-9])(?:[a-z]-\d{2}[-_][a-z]|[a-z]-\d{2}\.(?:md|json|csv|txt)\b|p0\d[-_](?:component|startup|threshold)\w*\b|r\d+-[a-z]\d+\b)/i;

function walk(dir, out) {
  let entries;
  try {
    entries = readdirSync(dir);
  } catch {
    return;
  }
  for (const name of entries) {
    if (dir === "benchmarks" && ["runs", "runs-warm"].includes(name)) continue;
    const p = join(dir, name);
    const st = statSync(p);
    if (st.isDirectory()) walk(p, out);
    else if (/\.(rs|ts|tsx|mjs|js|ps1|yml|yaml|md)$/.test(name)) out.push(p);
  }
}

const files = [];
for (const r of roots) {
  if (!existsSync(r)) {
    console.error(`internal jargon: missing scan root ${r}`);
    process.exit(1);
  }
  walk(r, files);
}
for (const f of rootFiles) if (existsSync(f)) files.push(f);
if (files.length === 0) {
  console.error("internal jargon: no files were scanned");
  process.exit(1);
}
const hits = [];
for (const f of files) {
  const name = relative(process.cwd(), f).replace(/\\/g, "/");
  if (excluded.has(name)) continue;
  if (artifactReference.test(name) || idRe.test(name)) hits.push(`${name}: public file name`);
  const text = readFileSync(f, "utf8");
  const markdown = f.endsWith(".md");
  text.split(/\r?\n/).forEach((line, i) => {
    if (artifactReference.test(line)) {
      hits.push(`${name}:${i + 1}:${line.trim()}`);
      return;
    }
    // Source files: only flag comment-ish lines to avoid string literals in
    // tests. Markdown is release-facing prose, so every line is checked.
    if (
      !markdown &&
      !/^\s*(\/\/|\/\*|\*|#|<!--)/.test(line) &&
      !/\/\//.test(line) &&
      !/\/\*/.test(line)
    ) {
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
