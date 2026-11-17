import { describe, expect, it } from "vitest";
import { buildReportAiRequest } from "../lib/aiNarrative";
import type { FullCleanupReport } from "../types";

describe("buildReportAiRequest", () => {
  it("maps full cleanup report fields and top failed paths", () => {
    const r: FullCleanupReport = {
      app_name: "Demo",
      dry_run: false,
      backup_dir: "C:\\bak",
      uninstall_ok: true,
      uninstall_message: "ok",
      deleted: 3,
      failed: 2,
      skipped: 1,
      aborted: false,
      restore_point_ok: true,
      restore_point_msg: "",
      errors: [],
      item_details: [
        { path: "C:\\ok", kind: "file", status: "deleted", message: "" },
        { path: "C:\\f1", kind: "file", status: "failed", message: "" },
        { path: "C:\\f2", kind: "registry", status: "Failed", message: "" },
        { path: "C:\\f3", kind: "dir", status: "failed", message: "" },
        { path: "C:\\f4", kind: "dir", status: "failed", message: "" },
        { path: "C:\\f5", kind: "dir", status: "failed", message: "" },
        { path: "C:\\f6", kind: "dir", status: "failed", message: "" },
      ],
    };
    const req = buildReportAiRequest(r);
    expect(req.appName).toBe("Demo");
    expect(req.deleted).toBe(3);
    expect(req.failed).toBe(2);
    expect(req.backupDir).toBe("C:\\bak");
    expect(req.topFailed).toEqual(["C:\\f1", "C:\\f2", "C:\\f3", "C:\\f4", "C:\\f5"]);
  });
});
