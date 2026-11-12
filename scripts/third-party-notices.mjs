import { createHash } from "node:crypto";
import { readFile, readdir } from "node:fs/promises";
import path from "node:path";

const noticeName = /^(?:licen[cs]es?|copying|copyright|notices?|unlicense|third[-_ ]party[-_ ](?:notices?|licen[cs]es?))(?:$|[-_. ])/i;
const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
const byName = (a, b) => a < b ? -1 : a > b ? 1 : 0;

async function noticeFiles(directory, relative = "") {
  const names = [];
  for (const entry of await readdir(path.join(directory, relative), { withFileTypes: true })) {
    const name = relative ? `${relative}/${entry.name}` : entry.name;
    if (entry.isFile() && noticeName.test(entry.name)) names.push(name);
    if (entry.isDirectory() && entry.name !== "node_modules" && entry.name !== ".git") {
      names.push(...await noticeFiles(directory, name));
    }
  }
  return names;
}

async function readNotice(file) {
  const content = new TextDecoder("utf-8", { fatal: true }).decode(await readFile(file));
  if (!content.trim() || content.includes("\0")) throw new Error(`Invalid notice file: ${file}`);
  return content.replaceAll("\r\n", "\n");
}

function within(directory, file) {
  const relative = path.relative(directory, path.resolve(directory, file));
  if (!relative || relative === ".." || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) {
    throw new Error(`Notice file must be inside its package: ${file}`);
  }
  return relative.split(path.sep).join("/");
}

export async function collectNoticeInputs(root, metadata, reachable, npm) {
  const supplementDir = path.join(root, "scripts", "license-sources");
  const supplements = JSON.parse(await readFile(path.join(supplementDir, "sources.json"), "utf8"));
  const cargo = metadata.packages
    .filter((pkg) => pkg.source && reachable.has(`${pkg.name}@${pkg.version}`))
    .map((pkg) => ({
      ecosystem: "cargo",
      name: pkg.name,
      version: pkg.version,
      license: pkg.license,
      licenseFile: pkg.license_file,
      directory: path.dirname(pkg.manifest_path),
    }));
  const installed = await Promise.all(npm.map(async ({ name, version, packagePath }) => {
    const directory = path.join(root, within(root, packagePath));
    const pkg = JSON.parse(await readFile(path.join(directory, "package.json"), "utf8"));
    if (pkg.name !== name || pkg.version !== version) {
      throw new Error(`Installed npm package differs from the lockfile: ${name}@${version}. Run npm ci.`);
    }
    return { ecosystem: "npm", name, version, license: pkg.license, directory };
  }));

  return Promise.all([...cargo, ...installed].map(async (pkg) => {
    const key = `${pkg.ecosystem}:${pkg.name}@${pkg.version}`;
    const extra = supplements.packages[key];
    const paths = new Set(await noticeFiles(pkg.directory));
    if (pkg.licenseFile) paths.add(within(pkg.directory, pkg.licenseFile));
    const documents = await Promise.all([...paths].sort(byName).map(async (name) => ({
      name,
      text: await readNotice(path.join(pkg.directory, name)),
    })));
    if (extra?.input) {
      const inputPath = within(pkg.directory, extra.input.path);
      if (hash(await readFile(path.join(pkg.directory, inputPath))) !== extra.input.sha256) {
        throw new Error(`Supplemented binary differs from the reviewed upstream payload: ${key}/${inputPath}`);
      }
    }
    for (const source of extra?.documents ?? []) {
      const file = supplements.files[source.file];
      documents.push({
        name: file.name,
        source: source.url ?? file.url,
        text: await readNotice(path.join(supplementDir, within(supplementDir, source.file))),
      });
    }
    if (!documents.length) throw new Error(`No license or copyright text for ${key}; add the upstream notice for this version.`);
    if (!pkg.license && !pkg.licenseFile) throw new Error(`No license metadata for ${key}`);
    return { key, license: pkg.license ?? "See package license file", documents, source: extra?.source };
  }));
}

export function renderNotices(components, inputs) {
  const byKey = new Map();
  for (const input of inputs) {
    const existing = byKey.get(input.key);
    if (existing && JSON.stringify(existing) !== JSON.stringify(input)) {
      throw new Error(`Conflicting license notices for ${input.key}`);
    }
    byKey.set(input.key, input);
  }

  const texts = new Map();
  const entries = components.map(({ name, version, purl }) => {
    const ecosystem = purl.startsWith("pkg:npm/") ? "npm" : "cargo";
    const notice = byKey.get(`${ecosystem}:${name}@${version}`);
    if (!notice) throw new Error(`Missing license notice for ${purl}`);
    const links = notice.documents.map((doc) => {
      const digest = hash(doc.text);
      texts.set(digest, doc.text);
      const provenance = doc.source ? ` ([upstream](${doc.source}))` : "";
      return `[\`${doc.name}\`](#text-${digest})${provenance}`;
    });
    const source = notice.source ? ` [Corresponding source](${notice.source}).` : "";
    return `- ${name} ${version} (${purl}) — ${notice.license}. ${links.join(", ")}.${source}`;
  });

  const blocks = [...texts].sort(([a], [b]) => byName(a, b)).flatMap(([digest, text]) => {
    const fence = "`".repeat(Math.max(3, ...(text.match(/`+/g) ?? []).map((run) => run.length + 1)));
    return [`### Text ${digest}`, "", `${fence}text`, text.endsWith("\n") ? text.slice(0, -1) : text, fence, ""];
  });
  return [
    "# Third-party notices",
    "",
    "This file covers the locked Windows x64 Rust normal dependency graph and npm production packages used to build Remova. It includes notices for the Microsoft WebView2 loader linked by webview2-com-sys.",
    "",
    "License expressions are quoted from upstream package metadata. The links below point to the complete license and notice texts from each package or its versioned upstream source. Identical texts are printed once, with line endings normalized.",
    "",
    "Regenerate from the repository root after installing the locked dependencies with `npm ci` and `cargo fetch --locked --manifest-path src-tauri/Cargo.toml`: `node scripts/make-sbom.mjs --notices THIRD-PARTY-NOTICES.md`.",
    "",
    "## Components",
    "",
    ...entries,
    "",
    "## License and notice texts",
    "",
    ...blocks,
  ].join("\n");
}
