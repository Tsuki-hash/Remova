#!/usr/bin/env node
// One-shot: strip internal review/priority IDs from comments (keep behavior text).
import { readFileSync, writeFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const roots = ["src", "src-tauri/src", "src-tauri/tests"];
const patterns = [
  // Tagged ids at comment start or after colon/space, including compounds.
  /\b(?:REV-[A-Z]+-\d+(?:\/[A-Z]+-\d+)?|R2[123]-[A-Z]+-\d+(?:\/[A-Z]+-\d+)?|FN-\d+|CODE-\d+|ARCH-\d+|AR-\d+|PERF-\d+|FUNC-\d+|SEC-\d+|FE-P\d+[a-z]?|Q-[A-Z]\d+|S-R\d+-\d+|S-\d+|T-R\d+|F-R\d+|NEW-[A-Z]|P0-\d)\s*:\s*/g,
  /\b(?:REV-[A-Z]+-\d+|R2[123]-[A-Z]+-\d+|FN-\d+|CODE-\d+|ARCH-\d+|AR-\d+|PERF-\d+|FUNC-\d+|SEC-\d+|FE-P\d+[a-z]?|Q-[A-Z]\d+|S-R\d+-\d+|S-\d+|T-R\d+|F-R\d+|NEW-[A-Z]|P0-\d)\b/g,
  /\bP[0-3]\b/g,
];

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

function cleanLine(line) {
  const m = line.match(/^(\s*(?:\/\/|\/\*+|\*|#|<!--)\s*)/);
  if (!m) {
    // Trailing `//` comment
    const t = line.indexOf("//");
    if (t < 0) return line;
    const code = line.slice(0, t);
    let c = line.slice(t);
    for (const re of patterns) c = c.replace(re, "");
    c = c.replace(/(\s*\/\/)\s*:\s*/g, "$1 ");
    c = c.replace(/(\s*\/\/)\s+/g, "$1 ");
    return code + c;
  }
  let rest = line.slice(m[1].length);
  for (const re of patterns) rest = rest.replace(re, "");
  rest = rest.replace(/^\s*:\s*/, "");
  rest = rest.replace(/^\s+/, "");
  if (!rest || rest === "/" || rest === "*" || rest === "*/" || rest === "-->") {
    // Drop empty comment shells that only held the id.
    if (m[1].trim() === "//" || m[1].trim() === "#") return null;
    return m[1] + rest;
  }
  return m[1] + rest;
}

const files = [];
for (const r of roots) walk(r, files);
let changed = 0;
for (const f of files) {
  const text = readFileSync(f, "utf8");
  const lines = text.split(/\r?\n/);
  let dirty = false;
  const out = [];
  for (const line of lines) {
    const isComment =
      /^\s*(\/\/|\/\*|\*|#|<!--)/.test(line) ||
      (/\/\//.test(line) && !/https?:/.test(line));
    if (!isComment) {
      out.push(line);
      continue;
    }
    const next = cleanLine(line);
    if (next !== line) dirty = true;
    if (next !== null) out.push(next);
  }
  if (dirty) {
    writeFileSync(f, out.join("\n"), "utf8");
    changed += 1;
  }
}
console.log(`rewrote comments in ${changed} files`);
