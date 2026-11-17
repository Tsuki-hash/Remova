import { describe, expect, it } from "vitest";
import { buildAuditJson, buildAuditMarkdown, redactUserProfiles } from "../lib/exportAuditReport";
import type { FullCleanupReport } from "../types";

const report = (over: Partial<FullCleanupReport> = {}): FullCleanupReport => ({
  app_name: "Demo",
  dry_run: false,
  backup_dir: "C:\\Users\\Aaron\\AppData\\Roaming\\Remova\\Backup\\s1",
  uninstall_ok: true,
  uninstall_message: "",
  deleted: 1,
  failed: 0,
  skipped: 0,
  delayed: 0,
  aborted: false,
  restore_point_ok: true,
  restore_point_msg: "",
  errors: [],
  item_details: [
    { path: "C:\\Users\\Aaron\\AppData\\Local\\Demo\\cache", kind: "dir", status: "deleted", message: "" },
    { path: "C:\\Program Files\\Demo\\leftover.dll", kind: "file", status: "deleted", message: "" },
  ],
  ...over,
});

describe("redactUserProfiles", () => {
  it("reduces user-profile prefixes to a placeholder in both slash styles", () => {
    expect(redactUserProfiles("C:\\Users\\Aaron\\AppData\\Local\\x"))
      .toBe("C:\\Users\\<user>\\AppData\\Local\\x");
    expect(redactUserProfiles("C:/Users/Aaron/AppData/x"))
      .toBe("C:/Users/<user>/AppData/x");
  });

  it("keeps non-profile paths and the Users root itself untouched", () => {
    expect(redactUserProfiles("C:\\Program Files\\Demo")).toBe("C:\\Program Files\\Demo");
    expect(redactUserProfiles("C:\\Users")).toBe("C:\\Users");
  });

  it("handles the verbatim prefix without losing the path tail", () => {
    const out = redactUserProfiles("\\\\?\\C:\\Users\\Aaron\\AppData\\Local\\x");
    expect(out).toContain("<user>");
    expect(out).toContain("\\AppData\\Local\\x");
  });
});

describe("buildAuditJson", () => {
  it.each([
    ["D:\\Workspaces\\Profiles\\fixture-name", "d:/workspaces/profiles/Fixture-Name"],
    ["D:\\External Data\\profile[1]", "\\\\?\\D:\\External Data\\PROFILE[1]"],
    ["\\\\server\\Residents\\fixture-name", "\\\\?\\UNC\\SERVER\\Residents\\Fixture-Name"],
  ])("masks the exact Windows profile root %s in both formats", (profileRoot, alias) => {
    const path = alias + "\\.tool\\data.bin";
    const unrelated = alias + "-backup\\other.bin";
    const input = report({
      backup_dir: path,
      errors: ["Could not access \"" + profileRoot + "\""],
      item_details: [{ path, kind: "file", status: "failed", message: "path: " + path },
        { path: unrelated, kind: "file", status: "skipped", message: "" }],
    });
    const json = buildAuditJson(input, 0, null, { profileRoot }).report;
    expect(json).toMatchObject({ backup_dir: "<user>\\.tool\\data.bin", errors: ['Could not access "<user>"'] });
    expect(JSON.stringify(json)).toContain(JSON.stringify(unrelated).slice(1, -1));
    const markdown = buildAuditMarkdown(input, 0, null, { profileRoot });
    expect(markdown).toContain("&lt;user&gt;\\\\.tool\\\\data.bin");
    expect(markdown).not.toContain(alias.replaceAll("\\", "\\\\") + "\\\\.tool");
    expect(buildAuditJson(input, 0, null, { profileRoot, includeUserPaths: true }).report).toHaveProperty("backup_dir", path);
    expect(buildAuditMarkdown(input, 0, null, { profileRoot, summaryOnly: true })).not.toContain("data.bin");
  });

  it("omits the active uninstall receipt from both detailed export modes", () => {
    for (const includeUserPaths of [false, true]) {
      expect(buildAuditJson(report({ uninstall_review: 42 }), 0, null, { includeUserPaths }).report)
        .not.toHaveProperty("uninstall_review");
    }
  });

  it("redacts every string in the report and records review evidence", () => {
    const out = buildAuditJson(report(), 2, true) as Record<string, unknown>;
    const text = JSON.stringify(out);
    expect(text).not.toContain("Aaron");
    expect(out.review_rounds).toBe(2);
    expect(out.review_converged).toBe(true);
    expect(out.kind).toBe("remova-audit-report");
  });

  it("keeps paths and raw errors out of summaries even with full paths enabled", () => {
    const path = String.raw`D:\Relocated\private\data.bin`;
    const input = report({
      app_name: path, app_key: path, backup_dir: path, backup_verified: false,
      uninstall_message: path, restore_point_msg: path, errors: [`${path}: delete failed`],
    });
    const options = { summaryOnly: true, includeUserPaths: true };
    const summary = buildAuditJson(input, 0, null, options);
    expect(summary.report).toMatchObject({ backup_present: true, backup_verified: false, error_count: 1 });
    expect(JSON.stringify(summary)).not.toContain("Relocated");
    expect(buildAuditMarkdown(input, 0, null, options)).not.toContain("Relocated");
    expect(buildAuditMarkdown(input, 0, null)).toContain(`Backup: unverified · ${path.replaceAll("\\", "\\\\")}`);
    expect(buildAuditMarkdown(report({ backup_verified: true }), 0, null)).toContain("Backup: verified");
    expect(buildAuditJson(input, 0, null, { includeUserPaths: true }).report).toEqual({ ...input, uninstall_status: "ok" });
  });
});

it.each([
  { history_save_failed: true },
  { errors: ["cleanup history could not be saved: private path"] },
])("preserves cleanup history warnings in audit summaries (%j)", (over) => {
  const input = report(over);
  expect(buildAuditJson(input, 0, null, { summaryOnly: true }).report)
    .toHaveProperty("history_save_failed", true);
  for (const summaryOnly of [false, true]) {
    expect(buildAuditMarkdown(input, 0, null, { summaryOnly }))
      .toContain("- Cleanup history could not be saved.");
  }
  expect(buildAuditMarkdown(input, 0, null, { summaryOnly: true })).not.toContain("private path");
});

it.each([
  [{ uninstall_ok: true, uninstall_message: "cleanup cancelled", aborted: true }, "ok", "ok"],
  [{ uninstall_ok: false, uninstall_message: "skipped" }, "skipped", "skipped"],
  [{ uninstall_ok: false, uninstall_message: "no uninstall string" }, "skipped", "skipped"],
  [{ uninstall_ok: false, uninstall_message: "no items", aborted: true }, "skipped", "skipped"],
  [{ uninstall_ok: false, uninstall_message: "backup failed", aborted: true }, "not_run", "not run"],
  [{ uninstall_ok: false, uninstall_message: "system reboot pending; uninstall skipped", aborted: true }, "not_run", "not run"],
  [{ uninstall_ok: false, dry_run: true }, "not_run", "not run (dry run)"],
  [{ uninstall_ok: false, uninstall_message: "uninstaller exited with Some(1603)", aborted: true,
    errors: ["official uninstaller failed; leftover cleanup skipped"] }, "failed", "failed"],
] as const)("preserves the official uninstaller outcome for %j", (over, status, label) => {
  const input = report({ ...over, ...("errors" in over ? { errors: [...over.errors] } : {}) } as Partial<FullCleanupReport>);
  for (const summaryOnly of [false, true]) {
    expect(buildAuditJson(input, 0, null, { summaryOnly }).report).toHaveProperty("uninstall_status", status);
    expect(buildAuditMarkdown(input, 0, null, { summaryOnly })).toContain(`- Uninstall: ${label}\n`);
  }
});

it("keeps pipes, newlines and markup in a single Markdown table row", () => {
  const input = report({
    app_name: "Example\r\n| injected",
    item_details: [{ kind: "registry", status: "deleted", path: String.raw`HKCU\Software\Vendor|Entry`,
      message: "First\r\n<script>alert(1)</script> & next\rline" }],
  });
  const output = buildAuditMarkdown(input, 0, null);
  expect(output).toContain("# Remova audit — Example ↵ &#124; injected");
  const row = output.split("\n").find(line => line.startsWith("| registry"));
  expect(row?.split("|")).toHaveLength(6);
  expect(row).toContain(String.raw`HKCU\\Software\\Vendor&#124;Entry`);
  expect(row).toContain("First ↵ &lt;script&gt;alert(1)&lt;/script&gt; &amp; next ↵ line");
});

it("redacts profile names beyond the canonical Users spelling", async () => {
  const { redactUserProfiles } = await import("../lib/exportAuditReport");
  const raw = String.raw;
  // Case-insensitive segment.
  expect(redactUserProfiles(raw`C:\users\Aaron\AppData`)).toBe(raw`C:\users\<user>\AppData`);
  // UNC share keeps the server, loses the profile.
  expect(redactUserProfiles(raw`\\host\Users\Aaron\data`)).toBe(raw`\\host\Users\<user>\data`);
  // Relocated profile root.
  expect(redactUserProfiles(raw`D:\Profiles\Aaron\x`)).toBe(raw`D:\Profiles\<user>\x`);
  // A path that merely contains "users" as a substring stays untouched.
  expect(redactUserProfiles(raw`C:\tools\userspace\bin`)).toBe(raw`C:\tools\userspace\bin`);
});
