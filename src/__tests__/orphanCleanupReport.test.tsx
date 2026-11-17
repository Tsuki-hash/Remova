// @vitest-environment jsdom
import { useState } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { OrphanPage } from "../components/OrphanPage";
import { api } from "../lib/api";
import { getToasts, toast } from "../lib/toast";
import { setLang, t } from "../i18n";
import type { CleanupItem, FullCleanupReport } from "../types";

vi.mock("../lib/api", () => ({ api: {
  beginAssociationScan: vi.fn(async () => 101),
  cancelAssociationScan: vi.fn(async () => {}),
  associationScanProgress: vi.fn(async () => null),
  orphanScan: vi.fn(), fullCleanup: vi.fn(),
} }));
vi.mock("../lib/confirm", () => ({ requestConfirmEx: vi.fn(async () => ({ ok: true, checked: false })) }));

const keptPath = "C:\\Apps\\Reused";
const removedPath = "C:\\Apps\\OldCache";
const ownerChanged = "orphan directory now has an installed owner; scan again";
const ownerUnverified = "orphan ownership could not be verified; scan again";
const item = (path: string): CleanupItem => ({ path, kind: "dir", score: 90,
  confidence: "confirmed", risk: "low", reason: "", evidence: [] });
const complete = { status: "complete" as const, roots_scanned: 4, failed_roots: [] };

function Harness() {
  const [report, setReport] = useState<FullCleanupReport | null>(null);
  return <OrphanPage lastReport={report} onLastReport={setReport} />;
}

beforeEach(() => { vi.resetAllMocks(); vi.useFakeTimers(); });
afterEach(() => { cleanup(); toast.clear(); vi.useRealTimers(); setLang("zh"); });

const cases = (["zh", "en"] as const).flatMap(lang =>
  (["owner", "unverified", "mixed"] as const).flatMap(outcome =>
    (["empty", "partial", "error"] as const).map(refresh => ({ lang, outcome, refresh }))));

it.each(cases)("keeps orphan cleanup details after $outcome and a $refresh refresh in $lang", async ({ lang, outcome, refresh }) => {
  setLang(lang);
  const items = outcome === "mixed" ? [item(keptPath), item(removedPath)] : [item(keptPath)];
  vi.mocked(api.orphanScan).mockResolvedValueOnce({ items, omitted: 0, completeness: complete,
    cleanup_allowed: true, scope_revision: 73 });
  if (refresh === "error") vi.mocked(api.orphanScan).mockRejectedValueOnce(new Error("refresh unavailable"));
  else vi.mocked(api.orphanScan).mockResolvedValueOnce({ items: [], omitted: 0,
    completeness: refresh === "empty" ? complete : { status: "partial", roots_scanned: 3, failed_roots: ["inventory:HKCU"] },
    cleanup_allowed: refresh === "empty", scope_revision: 74 });
  const message = outcome === "unverified" ? ownerUnverified : ownerChanged;
  const report: FullCleanupReport = { app_name: t().orphanScan, dry_run: false, backup_dir: "",
    uninstall_ok: false, uninstall_message: "", deleted: outcome === "mixed" ? 1 : 0,
    failed: 0, skipped: 1, delayed: 0, aborted: false, restore_point_ok: false, restore_point_msg: "",
    errors: [], item_details: [{ path: keptPath, kind: "dir", status: "skipped", message },
      ...(outcome === "mixed" ? [{ path: removedPath, kind: "dir", status: "deleted", message: "" }] : [])] };
  vi.mocked(api.fullCleanup).mockResolvedValueOnce(report);
  render(<Harness />);
  await act(async () => {});
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: `${t().cleanup} (${items.length})` })); });
  expect(api.fullCleanup).toHaveBeenCalledExactlyOnceWith(expect.anything(), items,
    expect.objectContaining({ cleanup_source: "orphan", scope_revision: 73, backup_enabled: false }));
  expect(api.orphanScan).toHaveBeenCalledTimes(2);
  expect(getToasts().length).toBeGreaterThan(0);
  await act(async () => { await vi.advanceTimersByTimeAsync(8000); });
  expect(getToasts()).toHaveLength(0);
  const result = screen.getByRole("region", { name: lang === "zh" ? "上次孤立清理结果" : "Last orphan cleanup result" });
  expect(within(result).getByText(t().cleanupOutcome(report.deleted, 0, 1, 0))).toBeTruthy();
  expect(within(result).getByText(t().reportNextSkipped)).toBeTruthy();
  const summary = within(result).getByText(`${t().reportSkipped} · 1`, { selector: "summary" });
  summary.focus();
  expect(document.activeElement).toBe(summary);
  fireEvent.click(summary);
  expect(summary.parentElement?.hasAttribute("open")).toBe(true);
  expect(within(result).getByText(keptPath)).toBeTruthy();
  expect(within(result).getByText(outcome === "unverified" ? t().backendOrphanOwnerUnverified : t().backendOrphanOwnerChanged)).toBeTruthy();
  expect(within(result).queryByRole("button", { name: new RegExp(t().retryFailedItems) })).toBeNull();
  expect(screen.queryByRole("button", { name: new RegExp(`^${t().cleanup}( \\([0-9]+\\))?$`) })).toBeNull();
  expect(screen.queryByRole("checkbox", { name: keptPath })).toBeNull();
  if (refresh === "empty") expect(screen.getByText(t().orphanScanEmpty)).toBeTruthy();
  if (refresh === "partial") expect(screen.getByText(t().scanCompletenessPartial)).toBeTruthy();
  if (refresh === "error") expect(screen.queryByText(t().orphanScanEmpty)).toBeNull();
  expect(api.fullCleanup).toHaveBeenCalledOnce();
});
