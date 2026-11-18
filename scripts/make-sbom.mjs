#!/usr/bin/env node
// Generate a CycloneDX 1.5 SBOM for the shipped application: the Rust side
// covers the normal dependency graph for the Windows x64 release target
// (per `cargo tree`, excluding lockfile-only packages), and
// the npm side covers production dependencies only — dev tooling never
// reaches the bundle. Deterministic: components are sorted and the serial
// number is a stable SHA-256 over the component list, so the same tree
// always produces the same SBOM bytes.
// Usage: node scripts/make-sbom.mjs [--out <path>] [--notices <path> | --check-notices]
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { collectNoticeInputs, renderNotices } from "./third-party-notices.mjs";

const RELEASE_TARGET = "x86_64-pc-windows-msvc";

const RELEASE_FEATURES = "tauri/custom-protocol";

/** The normal edges for the same target and feature set used by Tauri. */
function windowsReleaseCargoPackages(cwd) {
  const tree = execFileSync("cargo", [
    "tree", "--locked", "--target", RELEASE_TARGET, "--features", RELEASE_FEATURES,
    "--edges", "normal", "--prefix", "none", "--format", "{p}",
  ], { cwd, encoding: "utf8" });
  const packages = new Map();
  for (const line of tree.trim().split(/\r?\n/)) {
    const match = /^(\S+) v(\S+)(?: |$)/.exec(line);
    if (!match) throw new Error(`Unexpected cargo tree package line: ${line}`);
    const [, name, version] = match;
    packages.set(`${name}@${version}`, { name, version, ecosystem: "cargo" });
  }
  return packages;
}

/** Production packages of a package-lock.json (v3 "packages" map), {name, version}. */
export function productionNpmPackages(lock) {
  const out = [];
  for (const [pkgPath, entry] of Object.entries(lock.packages ?? {})) {
    if (pkgPath === "" || entry.dev || entry.devOptional || !entry.version) continue;
    const name = pkgPath.split("node_modules/").pop();
    if (name) out.push({ name, version: entry.version, packagePath: pkgPath });
  }
  return out;
}

export function parsePackageLock(lock) {
  return productionNpmPackages(lock).map(({ name, version }) => ({ name, version }));
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
  const [lockText, pkg] = await Promise.all([
    readFile(path.join(root, "package-lock.json"), "utf8"),
    readFile(path.join(root, "package.json"), "utf8"),
  ]);
  const cargoDir = path.join(root, "src-tauri");
  const cargo = windowsReleaseCargoPackages(cargoDir);
  const lock = JSON.parse(lockText);
  const sbom = buildSbom({
    productName: "Remova",
    productVersion: JSON.parse(pkg).version,
    cargo: [...cargo.values()],
    npm: parsePackageLock(lock).map((c) => ({ ...c, ecosystem: "npm" })),
  });
  const noticesIdx = process.argv.indexOf("--notices");
  const checkNotices = process.argv.includes("--check-notices");
  if (noticesIdx > -1 || checkNotices) {
    // Metadata supplies license paths. Membership comes from the actual
    // release dependency graph above, without dev feature unification.
    const metadata = JSON.parse(execFileSync("cargo", [
      "metadata", "--format-version", "1", "--locked", "--filter-platform", RELEASE_TARGET,
      "--features", RELEASE_FEATURES,
    ], { cwd: cargoDir, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 }));
    const reachable = new Set(cargo.keys());
    const inputs = await collectNoticeInputs(root, metadata, reachable, productionNpmPackages(lock));
    const content = renderNotices(sbom.components, inputs);
    if (checkNotices) {
      const current = await readFile(path.join(root, "THIRD-PARTY-NOTICES.md"), "utf8");
      if (content !== current.replaceAll("\r\n", "\n")) {
        throw new Error("Third-party notices are stale. Run node scripts/make-sbom.mjs --notices THIRD-PARTY-NOTICES.md and review the changes.");
      }
      console.log(`Third-party notices OK (${inputs.packages.length} application components, ${inputs.installers.length} installer notices)`);
    }
    if (noticesIdx > -1) {
      await writeFile(path.resolve(process.argv[noticesIdx + 1]), content, "utf8");
    }
  }
  if (checkNotices && outIdx < 0 && noticesIdx < 0) return;
  await mkdir(path.dirname(out), { recursive: true });
  await writeFile(out, `${JSON.stringify(sbom, null, 2)}\n`, "utf8");
  console.log(`SBOM OK: ${out} (${sbom.components.length} components)`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(import.meta.filename)) {
  await main();
}
