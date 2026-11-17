import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const scripts = fileURLToPath(new URL("../", import.meta.url));
const hasPwsh = spawnSync("pwsh", ["-NoProfile", "-Command", "$PSVersionTable.PSVersion.Major"]).status === 0;
const sha = data => createHash("sha256").update(data).digest("hex");

test("local staging binds all build outputs and verifies the staged payload", { skip: !hasPwsh }, t => {
  const base = realpathSync(tmpdir());
  const root = mkdtempSync(join(base, "remova-local-release-"));
  t.after(() => {
    assert.equal(dirname(realpathSync(root)), base);
    rmSync(root, { recursive: true, force: true });
  });
  const checkout = join(root, "checkout");
  const put = (name, content) => {
    const path = join(checkout, name);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, content);
    return path;
  };
  const git = args => {
    const result = spawnSync("git", args, { cwd: checkout, encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
    return result.stdout.trim();
  };
  put(".gitignore", "src-tauri/target/\n.release-notes/\n");
  put("package-lock.json", "lock");
  put("src-tauri/Cargo.lock", "lock");
  put("src-tauri/tauri.conf.json", JSON.stringify({ version: "1.5.1", plugins: { updater: { pubkey: "" } } }));
  put("src-tauri/windows/installer.nsi", "fixture");
  put("src-tauri/windows/installer-hooks.nsh", "fixture");
  copyFileSync(join(scripts, "stage-local-release.ps1"), put("scripts/stage-local-release.ps1", ""));
  put("scripts/package-portable.ps1", String.raw`
$release = Join-Path $PSScriptRoot '../src-tauri/target/release'
$zip = Join-Path $release 'bundle/portable/Remova_1.5.1_x64-portable.zip'
New-Item -ItemType Directory -Path (Split-Path $zip) -Force | Out-Null
if ($env:TEST_LOCAL_RELEASE_PIN) {
    $changed = $false
    try { [IO.File]::WriteAllText((Join-Path $release 'remova.exe'), 'changed'); $changed = $true } catch { }
    if ($changed) { throw 'The executable was not pinned.' }
}
Copy-Item -LiteralPath (Join-Path $release 'remova.exe') -Destination $zip
if ($env:TEST_LOCAL_RELEASE_WRONG_ZIP) { [IO.File]::WriteAllText($zip, 'different executable') }
$global:LASTEXITCODE = 0
`);
  for (const name of ["smoke-portable", "make-checksums", "extract-release-notes"]) {
    put(`scripts/${name}.ps1`, "$global:LASTEXITCODE = 0");
  }
  put("scripts/make-sbom.mjs", "");
  put("scripts/acceptance/get-package-exe-reference.ps1", String.raw`
param($Artifacts, $CandidateVersion, $SevenZip, $WixDark)
$result = @{}
foreach ($pair in @(@('portable','Remova_1.5.1_x64-portable.zip'), @('nsis','Remova_1.5.1_x64-setup.exe'), @('msi','Remova_1.5.1_x64_zh-CN.msi'))) {
    $digest = (Get-FileHash -LiteralPath (Join-Path $Artifacts $pair[1])).Hash.ToLowerInvariant()
    $result[$pair[0]] = @{ package_sha256 = $digest; executable_sha256 = $digest }
}
$result | ConvertTo-Json -Depth 3
`);
  put(".release-notes/v1.5.1.md", "fixture download instructions");
  const outputs = ["remova.exe", "bundle/nsis/Remova_1.5.1_x64-setup.exe", "bundle/msi/Remova_1.5.1_x64_zh-CN.msi"];
  const expected = Object.fromEntries(outputs.map(name => {
    put(`src-tauri/target/release/${name}`, name);
    return [name, sha(name)];
  }));
  put("src-tauri/target/release/bundle/SHA256SUMS.txt", "fixture");
  put("src-tauri/target/release/bundle/SBOM.cdx.json", "{}");
  git(["init", "-q"]);
  git(["add", "."]);
  git(["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "Create fixture"]);
  const inputs = ["package-lock.json", "src-tauri/Cargo.lock", "src-tauri/tauri.conf.json", "src-tauri/windows/installer.nsi", "src-tauri/windows/installer-hooks.nsh"];
  const record = { schema_version: 2, commit: git(["rev-parse", "HEAD"]),
    inputs_sha256: Object.fromEntries(inputs.map(name => [name, sha(readFileSync(join(checkout, name)))])),
    outputs_sha256: expected };
  const cases = [
    ["matching", () => {}, {}, true],
    ["locked", () => {}, { TEST_LOCAL_RELEASE_PIN: "1" }, true],
    ["old-schema", r => { r.schema_version = 1; }, {}, false],
    ["missing-output", r => { delete r.outputs_sha256[outputs[1]]; }, {}, false],
    ["extra-output", r => { r.outputs_sha256.extra = sha("extra"); }, {}, false],
    ["wrong-input", r => { r.inputs_sha256[inputs[0]] = sha("other lock"); }, {}, false],
    ["wrong-zip", () => {}, { TEST_LOCAL_RELEASE_WRONG_ZIP: "1" }, false],
    ...outputs.map((name, index) => [`swapped-output-${index}`, () => {
      put(`src-tauri/target/release/${name}`, "different build with the same version");
    }, {}, false]),
  ];
  for (const [name, mutate, env, succeeds] of cases) {
    for (const output of outputs) put(`src-tauri/target/release/${output}`, output);
    const current = structuredClone(record);
    mutate(current);
    const evidence = join(root, `${name}.json`);
    writeFileSync(evidence, JSON.stringify(current));
    const result = spawnSync("pwsh", ["-NoProfile", "-NonInteractive", "-File", join(checkout, "scripts/stage-local-release.ps1"),
      "-Tag", "v1.5.1", "-BuildEvidence", evidence, "-OutDir", join(root, name), "-SevenZip", "fixture"], {
      encoding: "utf8", timeout: 30000, env: { ...process.env, TEST_LOCAL_RELEASE_PIN: "", TEST_LOCAL_RELEASE_WRONG_ZIP: "", ...env },
    });
    assert.equal(result.status, succeeds ? 0 : 1, `${name}\n${result.stdout}\n${result.stderr}`);
    if (succeeds) {
      const staged = JSON.parse(readFileSync(join(root, name, "build-inputs.json"), "utf8").replace(/^\uFEFF/, ""));
      assert.deepEqual(staged.outputs_sha256, expected);
    } else assert.match(result.stderr, /build record|inventory|build input changed|build outputs/i);
  }
});
