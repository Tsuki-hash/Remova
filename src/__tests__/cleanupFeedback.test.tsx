// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { ScopedScanPanel } from "../components/ScopedScanPanel";
import { cleanupFeedback } from "../lib/cleanupFeedback";
import { api } from "../lib/api";
import { toast } from "../lib/toast";
import { t } from "../i18n";
import type { CleanupItem, FullCleanupReport } from "../types";
vi.mock("../lib/api", () => ({ api: { fullCleanup: vi.fn(), openPath: vi.fn() } }));
vi.mock("../lib/confirm", () => ({ requestConfirmEx: vi.fn(async () => ({ ok: true, checked: true })) }));
vi.mock("../lib/toast", () => ({ toast: { info: vi.fn(), error: vi.fn(), success: vi.fn() } }));
afterEach(cleanup);
beforeEach(() => vi.clearAllMocks());
const report = (over: Partial<FullCleanupReport> = {}): FullCleanupReport => ({ app_name: "Tool",
  dry_run: false, backup_dir: "", uninstall_ok: false, uninstall_message: "", deleted: 2, failed: 0,
  skipped: 0, aborted: false, restore_point_ok: false, restore_point_msg: "", errors: [], item_details: [], ...over });
it.each([{ aborted: true }, { failed: 2, deleted: 0 }, { failed: 1, deleted: 1 }, { failed: 0 }])(
  "uses truthful severity for cleanup outcome %j", result => {
    const value = report(result);
    cleanupFeedback(value, t(), "fixture");
    expect(toast.error).toHaveBeenCalledTimes(value.aborted || value.failed ? 1 : 0);
    expect(toast.success).toHaveBeenCalledTimes(value.aborted || value.failed ? 0 : 1);
  });
it.each([true, false])("retains scoped cleanup feedback after refresh, aborted=%s", async aborted => {
  const item: CleanupItem = { path: "fixture", kind: "file", score: 90, confidence: "confirmed",
    risk: "low", reason: "fixture", evidence: [] };
  const scan = vi.fn().mockResolvedValueOnce([item]).mockResolvedValue([]);
  vi.mocked(api.fullCleanup).mockResolvedValue(report({ aborted, failed: 1 }));
  const onError = vi.fn();
  render(<ScopedScanPanel title="Tool" hint="" appName="Tool" scan={scan} cleanupSource="installer"
    onClose={vi.fn()} onError={onError} onLastReport={vi.fn()} />);
  await screen.findByText("fixture");
  vi.mocked(toast.success).mockClear();
  vi.mocked(toast.info).mockClear();
  fireEvent.click(screen.getByRole("button", { name: t().cleanup }));
  await waitFor(() => expect(toast.error).toHaveBeenCalled());
  await waitFor(() => expect(scan).toHaveBeenCalledTimes(aborted ? 1 : 2));
  expect(toast.success).not.toHaveBeenCalled();
  expect(toast.info).not.toHaveBeenCalled();
  expect(onError).toHaveBeenCalledTimes(aborted ? 1 : 0);
});
it("reports an open-path rejection without an unhandled promise", async () => {
  vi.mocked(api.openPath).mockRejectedValue(new Error("open denied"));
  const onError = vi.fn();
  const scan = vi.fn().mockResolvedValue([{ path: "fixture", kind: "file", score: 90,
    confidence: "confirmed", risk: "low", reason: "", evidence: [] }]);
  render(<ScopedScanPanel title="Tool" hint="" appName="Tool" scan={scan} cleanupSource="installer"
    onClose={vi.fn()} onError={onError} onLastReport={vi.fn()} />);
  fireEvent.click(await screen.findByRole("button", { name: t().openLocation }));
  await waitFor(() => expect(onError).toHaveBeenCalledOnce());
});
