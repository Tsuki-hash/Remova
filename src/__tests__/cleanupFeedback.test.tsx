// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { ScopedScanPanel } from "../components/ScopedScanPanel";
import { cleanupAccomplishedSomething, shouldRescanAfterUninstall, cleanupFeedback, cleanupNothingText } from "../lib/cleanupFeedback";
import { api } from "../lib/api";
import { toast } from "../lib/toast";
import { setLang, t, formatSize } from "../i18n";
import { backendText } from "../lib/backendText";
import type { CleanupItem, FullCleanupReport } from "../types";
vi.mock("../lib/api", () => ({ api: { fullCleanup: vi.fn(), openPath: vi.fn(),
  beginAssociationScan: vi.fn(async () => 101), cancelAssociationScan: vi.fn(async () => {}),
  associationScanProgress: vi.fn(async () => null) } }));
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

// `failed === 0` is not "success". A hand-picked selection the keep rules
// refused every row of is `deleted: 0, failed: 0, skipped: N, aborted: false`,
// and it was being reported as a green "ok 0 · failed 0".
it.each([
  { over: { deleted: 0, failed: 0, skipped: 3 }, done: false, why: "keep rules refused every row" },
  { over: { deleted: 0, failed: 0, skipped: 0 }, done: false, why: "there was nothing to delete" },
  { over: { deleted: 0, failed: 0, skipped: 0, uninstall_ok: true }, done: true, why: "the official uninstaller ran" },
  { over: { deleted: 0, failed: 0, delayed: 2 }, done: true, why: "deletes are pending reboot" },
  { over: { deleted: 1, failed: 0, skipped: 4 }, done: true, why: "something was deleted" },
  { over: { deleted: 0, failed: 1, skipped: 0 }, done: false, why: "a delete failed" },
  { over: { aborted: true, deleted: 0, failed: 0 }, done: false, why: "the run aborted" },
])("cleanupAccomplishedSomething is $done when $why", ({ over, done }) => {
  expect(cleanupAccomplishedSomething(report(over))).toBe(done);
});

it("explains a zero-delete cleanup instead of claiming success", () => {
  const L = t();
  expect(cleanupNothingText(report({ skipped: 3 }), L)).toContain("3");
  expect(cleanupNothingText(report({ skipped: 0 }), L)).not.toContain("0");
  // Both languages must say something, and they must differ.
  setLang("en");
  expect(cleanupNothingText(report({ skipped: 3 }), t())).not.toBe(
    cleanupNothingText(report({ skipped: 3 }), L),
  );
  setLang("zh");
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
  fireEvent.click(screen.getByRole("button", { name: new RegExp(t().cleanup) }));
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

it.each(["installer", "toolcache"] as const)("discards cancelled %s refresh targets and keeps cleanup disabled", async cleanupSource => {
  const item = { path: "old-target", kind: "file", score: 90, confidence: "confirmed",
    risk: "low", reason: "fixture", evidence: [] } as CleanupItem;
  let finish!: (items: CleanupItem[]) => void;
  const scan = vi.fn().mockResolvedValueOnce([item])
    .mockImplementationOnce(() => new Promise<CleanupItem[]>(resolve => { finish = resolve; }));
  const onError = vi.fn();
  render(<ScopedScanPanel title="Tool" hint="" appName="Tool" scan={scan} cleanupSource={cleanupSource}
    onClose={vi.fn()} onError={onError} onLastReport={vi.fn()} />);
  await screen.findByText("old-target");
  fireEvent.click(screen.getByRole("button", { name: t().scanStart }));
  await waitFor(() => expect(scan).toHaveBeenCalledTimes(2));
  expect(screen.queryByText("old-target")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: t().cancelScan }));
  await act(async () => { finish([item]); });
  const statusText = screen.getAllByRole("status").map(el => el.textContent).join(" ");
  expect(statusText).toContain(t().scanCancelledIncomplete);
  expect(screen.getByRole("button", { name: new RegExp(t().cleanup) }).hasAttribute("disabled")).toBe(true);
  expect(screen.queryByText(t().scanComplete(0))).toBeNull();
  expect(toast.success).toHaveBeenCalledTimes(1);
  expect(onError).not.toHaveBeenCalled();
});

/**
 * Renders the panel with the exact payload shapes the Rust scanners emit and
 * asserts the reason line localizes. The panel used to prefer `bucket`, and
 * none of the scanner buckets were in `backendText`, so every row rendered as
 * "no localized description yet" — invisible in unit tests that only passed
 * `reason`.
 */
it.each([
  { name: "toolcache whole dir", item: { path: "uv-cache-fixture", kind: "dir",
    score: 90, confidence: "confirmed", risk: "low", reason: "tool cache (dev)",
    evidence: [], bucket: "dev", size_kb: 446000 } },
  { name: "toolcache shared child", item: { path: "shared-child-fixture", kind: "dir",
    score: 50, confidence: "suspected", risk: "medium", reason: "shared tool cache",
    evidence: [], bucket: "dev" } },
  { name: "installer updater dir", item: { path: "updater-dir-fixture", kind: "dir",
    score: 50, confidence: "suspected", risk: "medium", reason: "updater cache",
    evidence: [], bucket: "updater_cache" } },
  { name: "installer package file", item: { path: "package-file-fixture", kind: "file",
    score: 50, confidence: "suspected", risk: "medium", reason: "possible installer package",
    evidence: [], bucket: "download_pkg" } },
] as const)("localizes the reason line for %s", async ({ item }) => {
  setLang("zh");
  const scan = vi.fn().mockResolvedValue([item]);
  render(<ScopedScanPanel title="T" hint="" appName="T" scan={scan} cleanupSource="installer"
    onClose={vi.fn()} onError={vi.fn()} onLastReport={vi.fn()} />);
  const L = t();
  await screen.findByText(item.path);
  // The row's own reason text must be present, and the fallback must not. A
  // bucket group header can legitimately carry the same label (updater_cache
  // rows sit under an "installer" header), so accept more than one match.
  const expected = backendText(item.reason, "scan");
  expect(expected).not.toBe(L.backendDetailUnavailable);
  expect(screen.getAllByText(expected, { exact: false }).length).toBeGreaterThan(0);
  expect(screen.queryByText(L.backendDetailUnavailable, { exact: false })).toBeNull();
  setLang("zh");
});

/**
 * The panel used to show only "N items selected" with no space estimate, no
 * select-all, and a cleanup button with no count — so a user facing 20+
 * preselected caches had to uncheck them one by one and could not see how much
 * would be freed.
 */
it("shows the space to be freed, a select count, and select-all", async () => {
  setLang("zh");
  const items: CleanupItem[] = [
    { path: "big-cache", kind: "dir", score: 90, confidence: "confirmed", risk: "low",
      reason: "tool cache (dev)", evidence: [], bucket: "dev", size_kb: 2_000_000 },
    { path: "small-cache", kind: "dir", score: 50, confidence: "suspected", risk: "medium",
      reason: "shared tool cache", evidence: [], bucket: "dev", size_kb: 4_096 },
    { path: "unmeasured", kind: "dir", score: 50, confidence: "suspected", risk: "medium",
      reason: "shared tool cache", evidence: [], bucket: "dev", size_kb: null },
  ];
  const scan = vi.fn().mockResolvedValue(items);
  render(<ScopedScanPanel title="T" hint="" appName="T" scan={scan} cleanupSource="toolcache"
    onClose={vi.fn()} onError={vi.fn()} onLastReport={vi.fn()} />);
  const L = t();
  await screen.findByText("big-cache");

  // Only the Confirmed/Low item is preselected, and the button says how many.
  const cleanupBtn = screen.getByRole("button", { name: new RegExp(L.cleanup) });
  expect(cleanupBtn.textContent).toContain("1");
  // The header reports the space for what is selected. An unmeasured item only
  // counts once it is actually selected — it is not selected yet, so no caveat.
  // The header reports the space for what is selected. The live region settles
  // after the selection stops changing (UX-15).
  await waitFor(() =>
    expect(screen.getByRole("status").textContent).toContain(formatSize(2_000_000)),
  );
  expect(screen.getByRole("status").textContent).not.toContain(L.planUnknownSize(1));
  // Rows carry a risk label and a confidence label.
  expect(screen.getAllByText(L.low).length).toBeGreaterThan(0);
  expect(screen.getAllByText(L.suspected).length).toBeGreaterThan(0);

  // Select-all checks everything, and now the unmeasured item is in scope.
  fireEvent.click(screen.getByRole("button", { name: L.orphanSelectAll }));
  expect(cleanupBtn.textContent).toContain("3");
  await waitFor(() =>
    expect(screen.getByRole("status").textContent).toContain(L.planUnknownSize(1)),
  );

  // Clear selection empties it and disables cleanup.
  fireEvent.click(screen.getByRole("button", { name: L.clearSelection }));
  expect(screen.getByRole("button", { name: new RegExp(L.cleanup) }).hasAttribute("disabled")).toBe(true);
  setLang("zh");
});

describe("shouldRescanAfterUninstall", () => {
  it("treats a 1641 abort as a completed uninstall but not a dry run", () => {
    expect(shouldRescanAfterUninstall({ dry_run: false, uninstall_ok: true })).toBe(true);
    expect(shouldRescanAfterUninstall({ dry_run: true, uninstall_ok: true })).toBe(false);
    expect(shouldRescanAfterUninstall({ dry_run: false, uninstall_ok: false })).toBe(false);
    expect(shouldRescanAfterUninstall({ dry_run: false })).toBe(false);
  });
});
