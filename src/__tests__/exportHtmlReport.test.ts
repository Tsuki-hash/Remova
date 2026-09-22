import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { exportHtmlReport } from "../lib/exportHtmlReport";
import type { FullCleanupReport } from "../types";

const L = {
  dryRunSummary: "演练",
  batchSummary: "清理",
  batchOk: "成功",
  batchFailed: "失败",
  batchSkipped: "跳过",
  reportKind: "类型",
  reportStatus: "结果",
  reportPath: "路径",
  reportMessage: "说明",
  reportAborted: "已中止",
  reportBackup: "备份",
};

function report(partial: Partial<FullCleanupReport> = {}): FullCleanupReport {
  return {
    app_name: "Demo <App>",
    dry_run: false,
    backup_dir: "C:\\bak",
    uninstall_ok: true,
    uninstall_message: "ok",
    deleted: 1,
    failed: 0,
    skipped: 0,
    aborted: false,
    restore_point_ok: true,
    restore_point_msg: "",
    errors: [],
    item_details: [
      {
        path: "C:\\evil<script>",
        kind: "file",
        status: "deleted",
        message: "a & b",
      },
    ],
    ...partial,
  };
}

type Captured = { html: string; download: string; clicked: boolean };

let captured: Captured;
const origCreateObjectURL = URL.createObjectURL;
const origRevokeObjectURL = URL.revokeObjectURL;
const origBlob = globalThis.Blob;

beforeEach(() => {
  captured = { html: "", download: "", clicked: false };

  class MockBlob {
    parts: unknown[];
    constructor(parts: unknown[], _opts?: unknown) {
      this.parts = parts;
    }
    async text() {
      return this.parts.map((p) => String(p)).join("");
    }
  }
  (globalThis as { Blob: unknown }).Blob = MockBlob;

  (URL as unknown as { createObjectURL: unknown }).createObjectURL = (b: unknown) => {
    const blob = b as { parts?: unknown[]; text?: () => Promise<string> };
    if (blob && typeof blob.text === "function") {
      void blob.text().then((t) => {
        captured.html = t;
      });
    } else if (blob?.parts) {
      captured.html = blob.parts.map((p) => String(p)).join("");
    }
    return "blob:remova-test";
  };
  (URL as unknown as { revokeObjectURL: unknown }).revokeObjectURL = () => {};

  (globalThis as { document: unknown }).document = {
    createElement: (_tag: string) => ({
      href: "",
      download: "",
      click() {
        captured.clicked = true;
        captured.download = this.download;
      },
    }),
  };
});

afterEach(() => {
  (URL as unknown as { createObjectURL: unknown }).createObjectURL = origCreateObjectURL;
  (URL as unknown as { revokeObjectURL: unknown }).revokeObjectURL = origRevokeObjectURL;
  if (origBlob) {
    (globalThis as { Blob: unknown }).Blob = origBlob;
  }
  delete (globalThis as { document?: unknown }).document;
  vi.restoreAllMocks();
});

describe("exportHtmlReport (real module)", () => {
  it("escapes path/message and triggers download with sanitized filename", async () => {
    exportHtmlReport(report(), L);
    // Blob text is resolved async in mock
    await Promise.resolve();
    await Promise.resolve();

    expect(captured.clicked).toBe(true);
    expect(captured.download).toBe("remova-report-Demo_App_.html");
    expect(captured.html).toContain("Demo &lt;App&gt;");
    expect(captured.html).toContain("C:\\evil&lt;script&gt;");
    expect(captured.html).toContain("a &amp; b");
    expect(captured.html).not.toContain("<script>");
    expect(captured.html).toContain("清理");
    expect(captured.html).toContain("备份: C:\\bak");
    // labels come from the passed strings, not hardcoded English.
    expect(captured.html).toContain("<th>类型</th>");
    expect(captured.html).toContain("<th>结果</th>");
    expect(captured.html).toMatch(/<html lang="(zh|zh-CN|en)">/);
  });

  it("shows dry-run label and aborted marker", async () => {
    exportHtmlReport(
      report({
        app_name: "X",
        dry_run: true,
        aborted: true,
        deleted: 0,
        skipped: 2,
        uninstall_message: "",
        backup_dir: "",
        item_details: [],
      }),
      L,
    );
    await Promise.resolve();
    await Promise.resolve();
    expect(captured.html).toContain("演练");
    expect(captured.html).toContain("已中止");
    expect(captured.download).toBe("remova-report-X.html");
  });
});
