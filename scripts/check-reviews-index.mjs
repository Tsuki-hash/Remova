#!/usr/bin/env node
// Docs index gate: every dated `.md` file under docs/reviews/ must be named in
// docs/reviews/README.md. Non-Markdown attachments in the dated folders are out
// of scope (the README index only lists reports).
//
// The README promises "本 README 的索引、版号一览和最新报告入口随新文件同步更新".
// That promise was broken repeatedly (the index fell behind newer reports),
// so the index is now *checked* rather than
// trusted. Matching is on the path relative to docs/reviews/, which is what the
// README's own links use — a basename alone would let one link cover two files.
//
// Usage:  node scripts/check-reviews-index.mjs [--root <repo root>]
// Exit 0 = index complete, or docs/ is absent from this checkout;
// exit 1 = files missing from the index.
//
// `docs/` is gitignored (see .gitignore), so a fresh CI checkout has no
// docs/reviews/ at all. That is not an index violation — it is the documented
// state of the public tree — so a missing directory SKIPs rather than fails.
// Wiring this into CI would otherwise turn a green pipeline red on a clean
// clone, which is exactly the failure mode that hid the disabled workflows.

import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const args = process.argv.slice(2);
const rootFlag = args.indexOf("--root");
const repoRoot =
  rootFlag === -1
    ? join(fileURLToPath(import.meta.url), "..", "..")
    : args[rootFlag + 1];

const reviewsDir = join(repoRoot, "docs", "reviews");
const indexPath = join(reviewsDir, "README.md");

if (!existsSync(reviewsDir)) {
  // Only skip when the whole docs tree is absent. A repo that tracks docs/ but
  // lost its reviews directory has a real problem and should still fail.
  if (!existsSync(join(repoRoot, "docs"))) {
    console.log(
      "SKIP: docs/ is not present in this checkout (docs/ is gitignored); " +
        "check-reviews-index is a local maintainer gate.",
    );
    process.exit(0);
  }
  console.error(`check-reviews-index: ${reviewsDir} does not exist`);
  process.exit(1);
}
if (!existsSync(indexPath)) {
  console.error(`check-reviews-index: ${indexPath} does not exist`);
  process.exit(1);
}

const index = readFileSync(indexPath, "utf8");

/** Recursively collect every .md file under a directory. */
function collectMarkdown(dir) {
  const found = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      found.push(...collectMarkdown(full));
    } else if (entry.endsWith(".md")) {
      found.push(full);
    }
  }
  return found;
}

// The index itself is not an entry in itself.
const files = collectMarkdown(reviewsDir)
  .filter((file) => file !== indexPath)
  .map((file) => relative(reviewsDir, file).split(sep).join("/"))
  .sort();

const missing = files.filter((rel) => !index.includes(rel));

if (missing.length > 0) {
  console.error(
    `check-reviews-index: ${missing.length} file(s) under docs/reviews/ are not named in docs/reviews/README.md:`,
  );
  for (const rel of missing) {
    console.error(`  - ${rel}`);
  }
  console.error(
    "Add a row (or a link) for each one; the README states that its index, round\n" +
      "table and latest-report links are kept in sync as files land.",
  );
  process.exit(1);
}

console.log(
  `check-reviews-index OK: all ${files.length} file(s) under docs/reviews/ appear in docs/reviews/README.md`,
);
