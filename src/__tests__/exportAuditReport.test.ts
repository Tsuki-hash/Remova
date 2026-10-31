import { describe, expect, it } from "vitest";
import { buildAuditJson, redactUserProfiles } from "../lib/exportAuditReport";
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
  it("redacts every string in the report and records review evidence", () => {
    const out = buildAuditJson(report(), 2, true) as Record<string, unknown>;
    const text = JSON.stringify(out);
    expect(text).not.toContain("Aaron");
    expect(out.review_rounds).toBe(2);
    expect(out.review_converged).toBe(true);
    expect(out.kind).toBe("remova-audit-report");
  });
});
