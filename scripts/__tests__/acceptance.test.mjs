import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const source = fileURLToPath(new URL("../acceptance/", import.meta.url));
const thresholds = fileURLToPath(new URL("../../benchmarks/performance-thresholds.md", import.meta.url));
const commit = "a".repeat(40);
const exeSha = "b".repeat(64);
const sha = (data) => createHash("sha256").update(data).digest("hex");
const hasPwsh = spawnSync("pwsh", ["-NoProfile", "-Command", "$PSVersionTable.PSVersion.Major"]).status === 0;
const run = (script, args, env = {}) =>
  spawnSync("pwsh", ["-NoProfile", "-NonInteractive", "-File", script, ...args], {
    encoding: "utf8", timeout: 30000, env: { ...process.env, ...env },
  });
const resultText = (result) => result.stdout + result.stderr + (result.error?.message ?? "");
const readJson = (file) => JSON.parse(readFileSync(file, "utf8").replace(/^\uFEFF/, ""));

function fixture(t) {
  const base = realpathSync(tmpdir());
  const root = mkdtempSync(join(base, "remova-acceptance-"));
  t.after(() => {
    assert.equal(dirname(realpathSync(root)), base);
    rmSync(root, { recursive: true, force: true });
  });
  return root;
}

function mockRunner(root, script, functions) {
  const target = join(root, script);
  copyFileSync(join(source, script), target);
  writeFileSync(join(root, "acceptance-common.ps1"),
    readFileSync(join(source, "acceptance-common.ps1"), "utf8") + "\n" + functions);
  return target;
}

const fakeWindows = String.raw`
function Confirm-AcceptanceVm { 'Fixture virtual machine' }
function Test-AcceptanceElevated { $env:TEST_ACCEPTANCE_ELEVATED -eq '1' }
function Get-AcceptanceWindows { @{family='windows-11';version='10.0.26200';architecture='x64'} }
function Get-AcceptanceWebViewVersion { '143.0.1.1' }
`;

test("acceptance records a passing bundle and rejects missing, changed, skipped or unevidenced scenarios", { skip: !hasPwsh }, (t) => {
  const root = fixture(t);
  const script = mockRunner(root, "run-acceptance.ps1", fakeWindows + String.raw`
function Read-Host([string]$Prompt) {
    switch -Regex ($Prompt) {
        'full path of the (nsis|msi|portable) executable' { "C:\fixture\$($Matches[1])\remova.exe"; break }
        'Observed' { if ($env:TEST_ACCEPTANCE_SKIP) { 'skip' } else { 'PASS' }; break }
        'Describe' { 'Observed the expected result with a disposable fixture'; break }
        'Path to' { $env:TEST_ACCEPTANCE_EVIDENCE; break }
        default { throw "Unexpected prompt: $Prompt" }
    }
}
function Get-AcceptanceLaunchObservation($Path, $Version, $ExpectedSha256) {
    if ($env:TEST_ACCEPTANCE_UNREADY) { throw 'The process did not show the enabled Software navigation button.' }
    @{ path = $Path; version = $Version; sha256 = $ExpectedSha256; pid = 4321; uia_ready = $true }
}
`);
  const artifacts = join(root, "artifacts");
  mkdirSync(artifacts);
  const names = ["Remova_1.4.1_x64-setup.exe", "Remova_1.4.1_x64_zh-CN.msi", "Remova_1.4.1_x64-portable.zip"];
  const makePackages = (mode) => {
    for (const name of names) writeFileSync(join(artifacts, name), name);
    if (mode === "missing") rmSync(join(artifacts, names[0]));
    if (mode === "wrong") writeFileSync(join(artifacts, names[0]), "other candidate");
  };
  writeFileSync(join(artifacts, "SBOM.cdx.json"), "{}");
  writeFileSync(join(artifacts, "SHA256SUMS.txt"), names.map((name) => sha(name) + "  " + name).join("\n"));
  const channels = ["nsis", "msi", "portable"];
  const reference = join(root, "package-exe-reference.json");
  writeFileSync(reference, JSON.stringify(Object.fromEntries(channels.map((channel, index) => [channel, {
    package_sha256: sha(names[index]), executable_sha256: sha(channel),
  }]))));
  const evidence = join(root, "evidence.txt");
  writeFileSync(evidence, "Fixture observation");
  for (const mode of ["client", "elevated", "missing", "wrong", "skip", "no-evidence", "no-ui"]) {
    makePackages(mode);
    const out = join(root, mode);
    const suite = mode === "elevated" ? mode : "client";
    const result = run(script, ["-Suite", suite, "-Artifacts", artifacts, "-Out", out,
      "-CandidateCommit", commit, "-CandidateVersion", "1.4.1", "-PackageExeReference", reference], {
      TEST_ACCEPTANCE_ELEVATED: suite === "elevated" ? "1" : "",
      TEST_ACCEPTANCE_SKIP: mode === "skip" ? "1" : "",
      TEST_ACCEPTANCE_EVIDENCE: mode === "no-evidence" ? "" : evidence,
      TEST_ACCEPTANCE_UNREADY: mode === "no-ui" ? "1" : "",
    });
    const passed = mode === "client" || mode === "elevated";
    assert.equal(result.status, passed ? 0 : 1, mode + "\n" + resultText(result));
    const report = readJson(join(out, readdirSync(out)[0], "acceptance-summary.json"));
    assert.equal(report.passed, passed);
    assert.equal(report.candidate_commit, commit);
    assert.equal(report.schema_version, 2);
    if (mode === "no-ui") assert.match(report.error, /enabled Software navigation/);
    if (passed) {
      assert.equal(report.checks.length, suite === "client" ? 35 : 24);
      assert.equal(report.artifacts.nsis.sha256, sha(names[0]));
      assert.deepEqual(Object.keys(report.executables), channels);
      for (const channel of channels) assert.deepEqual(report.executables[channel], {
        path: `C:\\fixture\\${channel}\\remova.exe`, version: "1.4.1", sha256: sha(channel), pid: 4321, uia_ready: true,
      });
      assert.equal(report.checks.find(({ id }) => id === "nsis-first-launch").evidence.sha256, sha("Fixture observation"));
    }
  }
});

test("the matrix verifies the installed program and always records a failed step", { skip: !hasPwsh }, (t) => {
  const root = fixture(t);
  const script = join(source, "run-install-matrix.ps1");
  const probe = join(root, "install-probe.ps1");
  writeFileSync(probe, String.raw`
param([string]$Source)
$ErrorActionPreference = 'Stop'
$ast = [Management.Automation.Language.Parser]::ParseFile($Source, [ref]$null, [ref]$null)
$fn = $ast.Find({param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Assert-MatrixInstall'}, $true)
. ([scriptblock]::Create($fn.Extent.Text))
$uninstallKey = 'mock'
$location = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$location = [IO.Path]::TrimEndingDirectorySeparator($location)
function Get-ItemProperty { @{DisplayVersion='1.4.1';InstallLocation=$location} }
function Get-Item { if ($fault -ne 'missing-exe') { @{VersionInfo=@{ProductVersion=$mockProductVersion}} } }
function Get-FileHash { @{Hash=$observedSha} }
function Test-MatrixAppLaunch { $fault -ne 'no-start' }
function Assert-MatrixResult($Step, $Condition, $Detail) { if (-not $Condition) { throw $Step } }
foreach ($fault in @('wrong-version','missing-exe','wrong-hash','no-start','healthy')) {
    $mockProductVersion = if ($fault -eq 'wrong-version') { '1.4.0' } else { '1.4.1' }
    $observedSha = if ($fault -eq 'wrong-hash') { 'wrong' } else { 'expected' }
    $failed = ''
    try { Assert-MatrixInstall 'upgrade' $location '1.4.1' 'expected' } catch { $failed = $_.Exception.Message }
    $expected = switch ($fault) {
        'wrong-version' { 'upgrade-version' }
        'missing-exe' { 'upgrade-version' }
        'wrong-hash' { 'upgrade-exe-sha256' }
        'no-start' { 'upgrade-launches' }
        'healthy' { '' }
    }
    if ($failed -ne $expected) { throw "$fault expected '$expected', got '$failed'" }
}
`);
  const result = run(probe, [script]);
  assert.equal(result.status, 0, resultText(result));

  const fake = mockRunner(root, "run-install-matrix.ps1", fakeWindows + String.raw`
function Get-Process { }
function Start-Process { @{ExitCode=17} }
`);
  const files = ["old.exe", "candidate.exe", "ai-config.json", "history.jsonl"].map((name) => join(root, name));
  for (const file of files) writeFileSync(file, "fixture");
  const out = join(root, "out");
  // Pass an array through a PowerShell invocation so -FixtureFiles remains two values.
  const wrapper = join(root, "matrix-main-probe.ps1");
  writeFileSync(wrapper, String.raw`
param([string]$Script,[string]$Root,[string]$Digest)
& $Script -Installer140 (Join-Path $Root 'old.exe') -CandidateInstaller (Join-Path $Root 'candidate.exe') -CandidateVersion '1.5.0' -InstallDir (Join-Path $Root 'install') -CandidateExeSha256 $Digest -Out (Join-Path $Root 'out') -FixtureFiles @((Join-Path $Root 'ai-config.json'),(Join-Path $Root 'history.jsonl')) -ConfirmReplaceInstalledApp
exit $LASTEXITCODE
`);
  const failed = run(wrapper, [fake, root, exeSha], { TEST_ACCEPTANCE_ELEVATED: "" });
  assert.equal(failed.status, 1, resultText(failed));
  const summary = readJson(join(out, "install-matrix.json"));
  assert.equal(summary.passed, false);
  assert.equal(summary.candidate_version, "1.5.0");
  assert.equal(summary.snapshot_rollback_required, true);
  assert.deepEqual(summary.steps.map(({ step, ok }) => [step, ok]),
    [["preflight", true], ["install-1.4.0", false], ["matrix-stopped", false]]);
});

test("the performance gate requires six limits, matching identities and three real readiness samples", { skip: !hasPwsh }, (t) => {
  const root = fixture(t);
  const startupFile = join(root, "startup.json");
  const componentsFile = join(root, "components.json");
  const limitFile = join(root, "thresholds.md");
  const thresholdText = readFileSync(thresholds, "utf8");
  const environment = { machine_sha256: "e".repeat(64), os_version: "10.0.26200", architecture: "x64", webview2_version: "143.0.1.1", elevated: false };
  const common = { schema_version: 1, candidate_commit: commit, sha256: exeSha, environment, thresholds_sha256: sha(thresholdText) };
  const startup = { ...common, kind: "remova-startup", measurement_mode: "cold", runs: [1, 2, 3].map((run) => ({
    run, boot_sha256: String(run).repeat(64), ready_observed: true, startup_ms: 100, working_set_mb: 10,
  })) };
  const components = {
    ...common, kind: "remova-components", fixture: { walk_entries: 4040, analyze_files: 200, backup_bytes: 1073741824 },
    metrics: { walk_worst_ms: 10, analyze_worst_ms: 10, cancel_worst_ms: 10, backup_ms: 10 },
  };
  const variants = [
    ["complete", 0, () => {}],
    ["over-limit", 1, (s, c) => { c.metrics.backup_ms = 60001; }],
    ["cold-startup-over-limit", 1, (s) => { s.runs[0].startup_ms = 3038.4494; }],
    ["warm-startup", 2, (s) => { s.measurement_mode = "warm"; }],
    ["same-boot", 2, (s) => { s.runs[1].boot_sha256 = s.runs[0].boot_sha256; }],
    ["empty-startup", 2, (s) => { s.runs = []; }],
    ["false-ready", 2, (s) => { s.runs[0].ready_observed = false; }],
    ["bogus-ready", 2, (s) => { s.runs[0].ready_observed = "false"; }],
    ["missing-startup", 2, (s) => { delete s.runs[0].startup_ms; }],
    ["missing-component", 2, (s, c) => { c.metrics.backup_ms = null; }],
    ["different-commit", 2, (s, c) => { c.candidate_commit = "f".repeat(40); }],
    ["different-exe", 2, (s, c) => { c.sha256 = "f".repeat(64); }],
    ["different-machine", 2, (s, c) => { c.environment.machine_sha256 = "f".repeat(64); }],
    ["different-elevation", 2, (s, c) => { c.environment.elevated = true; }],
    ["missing-elevation", 2, (s) => { delete s.environment.elevated; }],
    ["elevation-must-be-boolean", 2, (s, c) => { c.environment.elevated = "false"; }],
    ["both-elevated", 0, (s, c) => { s.environment.elevated = c.environment.elevated = true; }],
    ["empty-limits", 2, () => ""],
    ["duplicate-limit", 2, () => thresholdText + "\n| Duplicate | backup_ms | 60000 |\n"],
  ];
  for (const [label, code, change] of variants) {
    const s = structuredClone(startup);
    const c = structuredClone(components);
    const text = change(s, c) ?? thresholdText;
    writeFileSync(startupFile, JSON.stringify(s));
    writeFileSync(componentsFile, JSON.stringify(c));
    writeFileSync(limitFile, text);
    const result = run(join(source, "measure-components.ps1"), [
      "-Startup", startupFile, "-Components", componentsFile, "-Thresholds", limitFile,
      "-CandidateCommit", commit, "-CandidateExeSha256", exeSha, "-AllowPartial",
    ]);
    assert.equal(result.status, code, label + "\n" + resultText(result));
  }
});
