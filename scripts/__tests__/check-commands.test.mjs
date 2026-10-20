import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const script = fileURLToPath(new URL("../check-commands.ps1", import.meta.url));
const pwsh = spawnSync("pwsh", ["-NoProfile", "-Command", "$PSVersionTable.PSVersion.Major"], {
  encoding: "utf8",
});
const havePwsh = pwsh.status === 0;

function fixture(t, { handlers, api, table }) {
  const root = mkdtempSync(join(tmpdir(), "remova-check-commands-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, "src-tauri", "src"), { recursive: true });
  mkdirSync(join(root, "src", "lib"), { recursive: true });
  writeFileSync(
    join(root, "src-tauri", "src", "lib.rs"),
    [
      "// @commands:start",
      ...table.map((name) => `// \`${name}\``),
      "// @commands:end",
      "let _ = generate_handler![",
      ...handlers.map((name) => `  ${name},`),
      "];",
    ].join("\n"),
  );
  writeFileSync(
    join(root, "src", "lib", "api.ts"),
    api.map((name) => `export const ${name} = () => invoke("${name}");`).join("\n"),
  );
  return root;
}

const run = (root) =>
  spawnSync("pwsh", ["-NoProfile", "-File", script, "-Root", root], { encoding: "utf8" });

test("backend-only exemption keeps list_restore_sessions honest", { skip: !havePwsh }, (t) => {
  const root = fixture(t, {
    handlers: ["alpha", "list_restore_sessions"],
    api: ["alpha"],
    table: ["alpha", "list_restore_sessions"],
  });
  const result = run(root);
  assert.equal(result.status, 0, result.stderr + result.stdout);
  assert.match(result.stdout, /OK: command table matches/);
});

test("module-qualified handlers are compared on the final segment", { skip: !havePwsh }, (t) => {
  const root = fixture(t, {
    handlers: ["commands::update::check_github_latest"],
    api: ["check_github_latest"],
    table: ["check_github_latest"],
  });
  const result = run(root);
  assert.equal(result.status, 0, result.stderr + result.stdout);
});

test("an api invoke with no handler fails the gate", { skip: !havePwsh }, (t) => {
  const root = fixture(t, {
    handlers: ["alpha"],
    api: ["alpha", "ghost"],
    table: ["alpha"],
  });
  const result = run(root);
  assert.equal(result.status, 1);
  assert.match(result.stdout + result.stderr, /ghost/);
});
