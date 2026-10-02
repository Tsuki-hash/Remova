import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const script = fileURLToPath(new URL("../check-internal-jargon.mjs", import.meta.url));
function fixture(run) {
  const root = mkdtempSync(join(tmpdir(), "remova-jargon-"));
  mkdirSync(join(root, "src"));
  try { run(root, join(root, "src", "sample.ts")); }
  finally { rmSync(root, { recursive: true, force: true }); }
}
const check = root => spawnSync(process.execPath, [script, "--check"], { cwd: root, encoding: "utf8" });

test("rejects every historical review-code family without changing source bytes", () => {
  fixture((root, file) => {
    for (const id of ["R2-11", "R-R7-02", "FE-R4-04", "R29-SEC-02", "R21-IO-02/BE-01", "REV-SEC-06"]) {
      const source = `// ${id}: protect the operation\nexport const safe = true;\n`;
      writeFileSync(file, source);
      const result = check(root);
      assert.equal(result.status, 1, id);
      assert.match(result.stderr, /sample.ts:1:/);
      assert.equal(readFileSync(file, "utf8"), source);
    }
  });
});

test("accepts explanatory comments and ordinary test string literals", () => {
  fixture((root, file) => {
    const source = '// Refuse untrusted ancestors.\nexport const sample = "R-R7-02";\n';
    writeFileSync(file, source);
    const result = check(root);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /0 hits/);
    assert.equal(readFileSync(file, "utf8"), source);
  });
});
