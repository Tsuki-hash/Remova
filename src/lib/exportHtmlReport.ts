import { escapeHtml } from "./format";
import type { FullCleanupReport } from "../types";

type Strings = {
  dryRunSummary: string;
  batchSummary: string;
  batchOk: string;
  batchFailed: string;
  batchSkipped: string;
};

export function exportHtmlReport(r: FullCleanupReport, L: Strings) {
  const rows = (r.item_details || [])
    .map(
      (d) =>
        `<tr><td>${escapeHtml(d.kind)}</td><td>${escapeHtml(d.status)}</td><td>${escapeHtml(d.path)}</td><td>${escapeHtml(d.message)}</td></tr>`,
    )
    .join("\n");
  const html = `<!doctype html><html lang="zh"><head><meta charset="utf-8"><title>Remova report — ${escapeHtml(r.app_name)}</title>
<style>body{font:14px/1.5 system-ui,sans-serif;margin:24px;color:#111}table{border-collapse:collapse;width:100%}td,th{border:1px solid #ccc;padding:6px 8px;text-align:left;vertical-align:top}th{background:#eef}</style>
</head><body><h1>Remova — ${escapeHtml(r.app_name)}</h1>
<p>${r.dry_run ? L.dryRunSummary : L.batchSummary} · ${L.batchOk}: ${r.deleted} · ${L.batchFailed}: ${r.failed} · ${L.batchSkipped}: ${r.skipped}${r.aborted ? " · aborted" : ""}</p>
<p>${escapeHtml(r.uninstall_message || "")}</p>
<p>backup: ${escapeHtml(r.backup_dir || "")}</p>
<table><tr><th>kind</th><th>status</th><th>path</th><th>message</th></tr>${rows}</table>
</body></html>`;
  const blob = new Blob([html], { type: "text/html;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `remova-report-${(r.app_name || "app").replace(/[^\w.-]+/g, "_")}.html`;
  a.click();
  URL.revokeObjectURL(url);
}
