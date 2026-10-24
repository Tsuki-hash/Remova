#!/usr/bin/env node
// Generate a CycloneDX 1.5 SBOM for the shipped application from the two
// lockfiles: src-tauri/Cargo.lock (everything the binary links) and
// package-lock.json (production npm dependencies only — dev tooling never
// reaches the bundle). Deterministic: components are sorted and the serial
// number is a stable SHA-256 over the component list, so the same tree
// always produces the same SBOM bytes.
// Usage: node scripts/make-sbom.mjs [--out <path>]
import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";

/** [package] entries of a Cargo.lock, as {name, version}. */
export function parseCargoLock(text) {
  const out = [];
  let name;
  for (const line of text.split("\n")) {
    const pkg = line.match(/^\[package\]/);
    if (pkg) {
      name = undefined;
      continue;
    }
    const n = line.match(/^name\s*=\s*"([^"]+)"/);
    if (n) name = n[1];
    const v = line.match(/^version\s*=\s*"([^"]+)"/);
    if (v && name) {
      out.push({ name, version: v[1] });
      name = undefined;
    }
  }
  return out;
}

/** Production packages of a package-lock.json (v3 "packages" map), {name, version}. */
export function parsePackageLock(lock) {
  const out = [];
  for (const [pkgPath, entry] of Object.entries(lock.packages ?? {})) {
    if (pkgPath === "" || entry.dev || entry.devOptional || !entry.version) continue;
    const name = pkgPath.split("node_modules/").pop();
    if (name) out.push({ name, version: entry.version });
  }
  return out;
}

/** Deterministic CycloneDX 1.5 document for the given components. */
export function buildSbom({ productName, productVersion, cargo, npm }) {
  const seen = new Set();
  const components = [...cargo, ...npm]
    .filter((c) => c.name !== "remova")
    .filter((c) => {
      const key = `${c.name}@${c.version}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    })
    .sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`))
    .map((c) => ({ type: "library", "bom-ref": `${c.name}@${c.version}`, name: c.name, version: c.version }));
  const serialInput = components.map((c) => c["bom-ref"]).join("\n");
  const serialNumber = `urn:uuid:${createHash("sha256").update(serialInput).digest("hex").replace(/(.{8})(.{4})(.{4})(.{4})(.{12}).*/, "$1-$2-$3-$4-$5")}`;
  return {
    bomFormat: "CycloneDX",
    specVersion: "1.5",
    serialNumber,
    version: 1,
    metadata: {
      component: { type: "application", "bom-ref": productName, name: productName, version: productVersion },
    },
    components,
  };
}

async function main() {
  const root = path.resolve(import.meta.dirname, "..");
  const outIdx = process.argv.indexOf("--out");
  const out = outIdx > -1 ? path.resolve(process.argv[outIdx + 1]) : path.join(root, "src-tauri", "target", "release", "bundle", "SBOM.cdx.json");
  const [cargoText, lockText, pkg] = await Promise.all([
    readFile(path.join(root, "src-tauri", "Cargo.lock"), "utf8"),
    readFile(path.join(root, "package-lock.json"), "utf8"),
    readFile(path.join(root, "package.json"), "utf8"),
  ]);
  const sbom = buildSbom({
    productName: "Remova",
    productVersion: JSON.parse(pkg).version,
    cargo: parseCargoLock(cargoText),
    npm: parsePackageLock(JSON.parse(lockText)),
  });
  await mkdir(path.dirname(out), { recursive: true });
  await writeFile(out, `${JSON.stringify(sbom, null, 2)}\n`, "utf8");
  console.log(`SBOM OK: ${out} (${sbom.components.length} components)`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(import.meta.filename)) {
  await main();
}
