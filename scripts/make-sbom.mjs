#!/usr/bin/env node
// Generate a CycloneDX 1.5 SBOM for the shipped application: the Rust side
// covers the normal dependency graph for the Windows x64 release target
// (per `cargo metadata`, excluding lockfile-only packages), and
// the npm side covers production dependencies only — dev tooling never
// reaches the bundle. Deterministic: components are sorted and the serial
// number is a stable SHA-256 over the component list, so the same tree
// always produces the same SBOM bytes.
// Usage: node scripts/make-sbom.mjs [--out <path>] [--notices <path>]
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";

const RELEASE_TARGET = "x86_64-pc-windows-msvc";

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

/**
 * Names reachable from the workspace members' normal dependency graphs, per a
 * `cargo metadata --format-version 1` document. Build-dependencies (proc
 * macros and codegen helpers) do not ship bytes in the binary and are
 * excluded, like dev-dependencies.
 */
export function reachableCargoNames(metadata) {
  // Traverse RESOLVED package IDs, not names. Walking by name merged every
  // version of a crate that appears more than once and invented reachability
  // edges that exist in no real build. The returned keys are
  // `name@version`, which is what the lockfile parser produces.
  // Resolved edges live in `resolve.nodes[].deps[]`, each naming an exact
  // package id. `packages[].dependencies` only holds version requirements, which
  // cannot distinguish two versions of the same crate.
  const depsOfId = new Map();
  for (const node of metadata.resolve?.nodes ?? []) {
    const deps = new Set();
    for (const dep of node.deps ?? []) {
      // `dep_kinds` lists how the edge is used. A normal dependency is encoded
      // as `kind: null` (not the string "normal"), so compare against null.
      const kinds = (dep.dep_kinds ?? []).map((k) => k.kind ?? "normal");
      if (kinds.length > 0 && !kinds.includes("normal")) continue;
      deps.add(dep.pkg);
    }
    depsOfId.set(node.id, deps);
  }
  const byId = new Map((metadata.packages ?? []).map((p) => [p.id, p]));
  const seen = new Set();
  const queue = [...(metadata.workspace_members ?? [])];
  while (queue.length) {
    const id = queue.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    for (const dep of depsOfId.get(id) ?? []) {
      if (!seen.has(dep)) queue.push(dep);
    }
  }
  return new Set(
    [...seen]
      .map((id) => byId.get(id))
      .filter((p) => p)
      .map((p) => `${p.name}@${p.version}`),
  );
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
      // Ecosystem-aware identity: the same name and version in crates.io and
      // npm are different components, not one.
      const key = `${c.ecosystem ?? "cargo"}:${c.name}@${c.version}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    })
    .sort((a, b) =>
      `${a.ecosystem ?? "cargo"}:${a.name}@${a.version}`.localeCompare(
        `${b.ecosystem ?? "cargo"}:${b.name}@${b.version}`,
      ),
    )
    .map((c) => {
      const ecosystem = c.ecosystem ?? "cargo";
      const purl =
        ecosystem === "npm"
          ? `pkg:npm/${encodeURIComponent(c.name).replace(/%2F/g, "/")}@${c.version}`
          : `pkg:cargo/${c.name}@${c.version}`;
      return {
        type: "library",
        "bom-ref": purl,
        name: c.name,
        version: c.version,
        purl,
      };
    });
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
  // The full lockfile lists every crate ever resolved in this tree; the SBOM
  // reports the normal graph resolved for the Windows release target.
  const metadata = JSON.parse(
    // `--locked` keeps the SBOM tied to the committed lockfile instead of a
    // silently re-resolved graph.
    execFileSync("cargo", ["metadata", "--format-version", "1", "--locked", "--filter-platform", RELEASE_TARGET], {
      cwd: path.join(root, "src-tauri"),
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
    }),
  );
  const reachable = reachableCargoNames(metadata);
  const sbom = buildSbom({
    productName: "Remova",
    productVersion: JSON.parse(pkg).version,
    cargo: parseCargoLock(cargoText)
      .filter((c) => reachable.has(`${c.name}@${c.version}`))
      .map((c) => ({ ...c, ecosystem: "cargo" })),
    npm: parsePackageLock(JSON.parse(lockText)).map((c) => ({ ...c, ecosystem: "npm" })),
  });
  await mkdir(path.dirname(out), { recursive: true });
  await writeFile(out, `${JSON.stringify(sbom, null, 2)}\n`, "utf8");
  console.log(`SBOM OK: ${out} (${sbom.components.length} components)`);
  const noticesIdx = process.argv.indexOf("--notices");
  if (noticesIdx > -1) {
    const header = [
      "# Third-party notices",
      "",
      "This list records the Windows x64 Rust dependency graph and npm production dependencies for Remova. Each component retains its upstream license.",
      "",
      "Regenerate from the repository root with `node scripts/make-sbom.mjs --notices THIRD-PARTY-NOTICES.md`.",
      "",
      "## Components",
      "",
    ];
    const components = sbom.components.map((c) => `- ${c.name} ${c.version} (${c.purl})`);
    await writeFile(path.resolve(process.argv[noticesIdx + 1]), [...header, ...components, ""].join("\n"), "utf8");
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(import.meta.filename)) {
  await main();
}
