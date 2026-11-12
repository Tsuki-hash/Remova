import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const script = fileURLToPath(new URL("../check-workflows-enabled.ps1", import.meta.url));
const workflows = [
  { name: "CI", path: ".github/workflows/ci.yml", state: "active" },
  { name: "Release", path: ".github/workflows/release.yml", state: "active" },
  { name: "Publish release draft", path: ".github/workflows/publish-release.yml", state: "active" },
];
const havePwsh = spawnSync("pwsh", ["-NoProfile", "-Command", "exit 0"]).status === 0;

function run({ apiStatus = 0, records = workflows, ci = "true" } = {}) {
  return spawnSync("pwsh", ["-NoProfile", "-NonInteractive", "-Command", `
    function gh {
      $global:LASTEXITCODE = 0
      if ($args[0] -eq 'api') {
        $global:LASTEXITCODE = [int]$env:TEST_WORKFLOW_STATUS
        $env:TEST_WORKFLOWS
      }
    }
    & $env:TEST_WORKFLOW_SCRIPT -Repo example/fixture
    exit $LASTEXITCODE
  `], {
    encoding: "utf8",
    env: {
      ...process.env,
      CI: ci,
      GH_TOKEN: "",
      TEST_WORKFLOW_STATUS: String(apiStatus),
      TEST_WORKFLOWS: JSON.stringify({ workflows: records }),
      TEST_WORKFLOW_SCRIPT: script,
    },
  });
}

test("workflow state gate verifies all three names and paths and fails closed in CI", { skip: !havePwsh }, () => {
  const active = run();
  assert.equal(active.status, 0, active.stderr);
  for (const records of [
    workflows.slice(0, 2),
    [...workflows.slice(0, 2), { ...workflows[2], state: "disabled_manually" }],
    [...workflows.slice(0, 2), { ...workflows[2], path: ".github/workflows/renamed.yml" }],
  ]) {
    const result = run({ records });
    assert.equal(result.status, 1, result.stderr + result.stdout);
    assert.match(result.stdout, /Publish release draft/);
  }
  const apiFailure = run({ apiStatus: 1 });
  assert.equal(apiFailure.status, 1, apiFailure.stderr);
  assert.match(apiFailure.stderr, /GitHub API failed/);
  const local = run({ apiStatus: 1, ci: "false" });
  assert.equal(local.status, 0, local.stderr);
  assert.match(local.stdout, /SKIP/);
});
