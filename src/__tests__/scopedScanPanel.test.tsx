// @vitest-environment jsdom
// the specialty scan panel's presentation layer had zero coverage — the
// four pre-existing cases only exercised feedback severity / refresh targets /
// openPath / cancel state. Everything below renders the panel against payloads
// copied from the Rust scanners (`toolcache.rs::push_item`, `installers.rs::
// push_item` / `push_dir_item`, `scanner/mod.rs::CleanupItem`) and asserts the
// presentation facts a user actually relies on.
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { ScopedScanPanel } from "../components/ScopedScanPanel";
import { api } from "../lib/api";
import { toast } from "../lib/toast";
import { setLang, t, formatSize } from "../i18n";
import { backendText } from "../lib/backendText";
import type { CleanupItem, FullCleanupReport, ScopedScanResult } from "../types";

vi.mock("../lib/api", () => ({ api: { fullCleanup: vi.fn(), openPath: vi.fn(async () => {}),
  beginAssociationScan: vi.fn(async () => 501), cancelAssociationScan: vi.fn(async () => {}),
  associationScanProgress: vi.fn(async () => null) } }));
vi.mock("../lib/confirm", () => ({ requestConfirmEx: vi.fn(async () => ({ ok: true, checked: false })) }));
vi.mock("../lib/toast", () => ({ toast: { info: vi.fn(), error: vi.fn(), success: vi.fn() } }));
afterEach(cleanup);
beforeEach(() => { vi.clearAllMocks(); setLang("zh"); });

/** `toolcache.rs::push_item` with `confirmed == true`. */
const toolCache = (path: string, domain: string, sizeKb: number | null = null): CleanupItem => ({
  path, kind: "dir", score: 90, confidence: "confirmed", risk: "low",
  reason: `tool cache (${domain})`,
  evidence: [{ code: "tool_cache", label: domain, weight: 50, detail: `${domain}\\${path}` }],
  shared: false, size_kb: sizeKb, bucket: domain,
});

/** `toolcache.rs::push_item` with `confirmed == false` (shared-root child). */
const sharedChild = (path: string, domain: string, sizeKb: number | null = null): CleanupItem => ({
  path, kind: "dir", score: 50, confidence: "suspected", risk: "medium",
  reason: "shared tool cache",
  evidence: [{ code: "tool_cache_shared", label: domain, weight: 30, detail: path }],
  shared: false, size_kb: sizeKb, bucket: domain,
});

/** `installers.rs::push_dir_item` (updater cache directory). */
const updaterDir = (path: string, sizeKb: number | null = null): CleanupItem => ({
  path, kind: "dir", score: 50, confidence: "suspected", risk: "medium",
  reason: "updater cache",
  evidence: [{ code: "updater_cache", label: "updater", weight: 35, detail: "updater_cache" }],
  shared: false, size_kb: sizeKb, bucket: "updater_cache",
});

/** `installers.rs::push_item` (a downloaded installer package). */
const installerFile = (path: string, sizeKb: number | null = null): CleanupItem => ({
  path, kind: "file", score: 50, confidence: "suspected", risk: "medium",
  reason: "possible installer package",
  evidence: [{ code: "installer_setup_name", label: path, weight: 30, detail: "downloads" }],
  shared: false, user_library: true, size_kb: sizeKb, bucket: "download_pkg",
});

/** PERF-A: a walk that hit a bound reports a lower bound, not a fake exact. */
const cappedToolCache = (path: string, kb: number): CleanupItem => ({
  ...toolCache(path, "dev", kb),
  size_capped: true,
});

const result = (items: CleanupItem[], over: Partial<ScopedScanResult> = {}): ScopedScanResult =>
  ({ items, omitted: 0, degraded: [], ...over });

const report = (over: Partial<FullCleanupReport> = {}): FullCleanupReport => ({
  app_name: "专项缓存清理", dry_run: false, backup_dir: "", uninstall_ok: false, uninstall_message: "",
  deleted: 1, failed: 0, skipped: 0, aborted: false, restore_point_ok: false, restore_point_msg: "",
  errors: [], item_details: [], ...over,
});

const panel = (scan: () => Promise<ScopedScanResult>) =>
  render(<ScopedScanPanel title={t().toolcacheTitle} hint="" appName={t().toolcacheTitle}
    scan={scan} cleanupSource="toolcache" onClose={vi.fn()} onError={vi.fn()} onLastReport={vi.fn()} />);

const installerPanel = (scan: () => Promise<ScopedScanResult>) =>
  render(<ScopedScanPanel title={t().installerTitle} hint={t().installerHint} appName={t().installerTitle}
    scan={scan} cleanupSource="installer" onClose={vi.fn()} onError={vi.fn()} onLastReport={vi.fn()} />);

it.each([
  { name: "tool cache whole dir", item: toolCache("C:\\Users\\u\\AppData\\Local\\uv\\cache", "dev", 4_608_000) },
  { name: "shared-root child", item: sharedChild("C:\\Users\\u\\AppData\\Local\\npm-cache\\_cacache", "dev") },
  { name: "updater cache dir", item: updaterDir("C:\\ProgramData\\Acme\\Downloads") },
  { name: "installer package file", item: installerFile("C:\\Users\\u\\Downloads\\Acme-3.2.msi", 84_000) },
] as const)("never falls back to the unavailable-reason copy for %s", async ({ item }) => {
  const L = t();
  panel(() => Promise.resolve(result([item])));
  await screen.findByText(item.path);
  const expected = backendText(item.reason, "scan");
  expect(expected).not.toBe(L.backendDetailUnavailable);
  expect(screen.getAllByText(expected, { exact: false }).length).toBeGreaterThan(0);
  expect(screen.queryByText(L.backendDetailUnavailable, { exact: false })).toBeNull();
  // The row's evidence code is localized too, so no raw English code leaks.
  for (const ev of item.evidence) {
    expect(L.scanEvidenceLabel(ev.code, ev.label)).not.toBe("");
  }
});

it("preselects only confirmed, non-shared, non-user-library rows", async () => {
  const L = t();
  const items = [
    toolCache("C:\\cache\\uv", "dev", 4_608_000),
    sharedChild("C:\\cache\\npm-child", "dev"),
    updaterDir("C:\\ProgramData\\Acme\\Downloads"),
    installerFile("C:\\Users\\u\\Downloads\\Acme-3.2.msi", 84_000),
  ];
  panel(() => Promise.resolve(result(items)));
  await screen.findByText("C:\\cache\\uv");
  expect(screen.getByRole("button", { name: new RegExp(L.cleanup) }).textContent).toContain("1");
  const boxes = screen.getAllByRole<HTMLInputElement>("checkbox");
  expect(boxes.map(b => b.checked)).toEqual([true, false, false, false]);
  // The one unselected-but-listed category is named in the summary bar.
  expect(screen.getAllByText(L.suspected).length).toBeGreaterThan(0);
});

it("reports the freed space for the selection and counts what it could not measure", async () => {
  const L = t();
  panel(() => Promise.resolve(result([
    toolCache("C:\\cache\\uv", "dev", 2_097_152),
    toolCache("C:\\cache\\cargo", "dev", 1_048_576),
    sharedChild("C:\\cache\\npm-child", "dev", null),
  ])));
  await screen.findByText("C:\\cache\\uv");
  // the summary is announced from a live region that settles after the
  // selection stops changing.
  await waitFor(() =>
    expect(screen.getByRole("status").textContent).toContain(
      L.orphanSelectedMeta(2, formatSize(3_145_728)),
    ),
  );
  // An unmeasured row is a caveat, not a silent zero.
  expect(screen.getByRole("status").textContent).not.toContain(L.planUnknownSize(1));
  fireEvent.click(screen.getByRole("button", { name: L.orphanSelectAll }));
  await waitFor(() =>
    expect(screen.getByRole("status").textContent).toContain(L.planUnknownSize(1)),
  );
  expect(screen.getByRole("status").textContent).toContain(formatSize(3_145_728));
});

it("groups rows by bucket and puts the largest measured item first in each group", async () => {
  const browser = toolCache("C:\\cache\\browser", "browser", 5_242_880);
  const tinyDev = toolCache("C:\\cache\\dev-tiny", "dev", 4_096);
  const bigDev = toolCache("C:\\cache\\dev-big", "dev", 2_097_152);
  const unmeasuredDev = sharedChild("C:\\cache\\dev-unknown", "dev", null);
  const { container } = panel(() => Promise.resolve(result([tinyDev, browser, bigDev, unmeasuredDev])));
  await screen.findByText("C:\\cache\\dev-big");
  const rows = screen.getAllByRole("listitem").map(el => el.textContent ?? "");
  const at = (needle: string) => rows.findIndex(p => p.includes(needle));
  // Groups are contiguous and alphabetical, so every dev row is one block.
  const devBlock = rows.slice(at("dev-big"), at("dev-unknown") + 1);
  expect(devBlock.filter(r => r.includes("dev-tiny")).length).toBe(1);
  // Inside the dev group the multi-GB entry leads the KB-sized one.
  expect(at("dev-big")).toBeLessThan(at("dev-tiny"));
  // An unmeasured row sinks to the bottom of its group instead of reading as 0 KB.
  expect(at("dev-unknown")).toBeGreaterThan(at("dev-tiny"));
  // One header per bucket, labelled with the localized bucket name. Headers are
  // presentation, not list items, so they are not announced as rows.
  const devHeader = backendText("dev", "scan");
  const headers = [...container.querySelectorAll('[role="presentation"]')];
  expect(headers.filter(el => el.textContent?.includes(devHeader)).length).toBe(1);
  expect(at("browser")).toBeLessThan(at("dev-big"));
});

it("shows an estimating placeholder instead of an empty list while the scan runs", async () => {
  let finish!: (value: ScopedScanResult) => void;
  panel(() => new Promise<ScopedScanResult>(resolve => { finish = resolve; }));
  expect(screen.getByText(t().scanEstimatingItems)).toBeTruthy();
  // The "nothing found" copy must not appear before the scan has answered.
  expect(screen.queryByText(t().scanEmptyNone)).toBeNull();
  await waitFor(() => finish(result([])));
  expect(screen.getByText(t().scanEmptyNone)).toBeTruthy();
  expect(screen.queryByText(t().scanEstimatingItems)).toBeNull();
});

// the empty state only branched on `items === null`, so a failed or
// cancelled scan left the list saying "measuring item sizes" forever while the
// progress bar above said the scan did not complete.
it("replaces the estimating placeholder with an error when the scan fails", async () => {
  const L = t();
  panel(() => Promise.reject(new Error("scan backend exploded")));
  await waitFor(() => expect(screen.queryByText(L.scanEstimatingItems)).toBeNull());
  expect(screen.getAllByText(L.scanFailedIncomplete).length).toBeGreaterThan(0);
  expect(screen.queryByText(L.scanEmptyNone)).toBeNull();
});

it("says a scan was cancelled instead of leaving the estimating placeholder", async () => {
  const L = t();
  panel(() => Promise.reject(new Error("scan:cancelled")));
  await waitFor(() => expect(screen.queryByText(L.scanEstimatingItems)).toBeNull());
  expect(screen.getAllByText(L.scanCancelledIncomplete).length).toBeGreaterThan(0);
});

// a cleanup IPC that never started used to leave the previous report and
// its still-clickable retry button on screen as if they described this attempt.
it("clears the previous report when a cleanup cannot even be submitted", async () => {
  const L = t();
  const items = [toolCache("C:\\cache\\uv", "dev", 2_097_152)];
  vi.mocked(api.fullCleanup).mockResolvedValueOnce(report({
    deleted: 1, failed: 1,
    item_details: [
      { path: "C:\\cache\\uv", kind: "dir", status: "deleted", message: "ok" },
      { path: "C:\\cache\\cargo", kind: "dir", status: "failed", message: "os error 32: being used by another process" },
    ],
  })).mockRejectedValueOnce(new Error("ipc closed"));
  panel(() => Promise.resolve(result(items)));
  await screen.findByText("C:\\cache\\uv");
  fireEvent.click(screen.getByRole("button", { name: L.orphanSelectAll }));
  fireEvent.click(screen.getByRole("button", { name: new RegExp(L.cleanup) }));
  await screen.findByText(L.cleanupResultTitle);
  expect(screen.getByRole("button", { name: new RegExp(L.retryFailedItems) })).toBeTruthy();

  fireEvent.click(screen.getByRole("button", { name: new RegExp(L.cleanup) }));
  await waitFor(() => expect(screen.queryByText(L.cleanupResultTitle)).toBeNull());
  expect(screen.queryByRole("button", { name: new RegExp(L.retryFailedItems) })).toBeNull();
  expect(screen.getByText("C:\\cache\\uv")).toBeTruthy();
});

it("distinguishes 'nothing found' from 'could not read the roots'", async () => {
  const L = t();
  // the backend now sends stable codes, not raw English prose.
  const degraded = ["unreadable_root:C:\\Users\\u\\AppData\\Local\\npm-cache", "downloads_root_missing"];
  panel(() => Promise.resolve(result([], { degraded })));
  await screen.findByText(L.scanDegradedHint);
  // Nothing was found AND the read was incomplete — say both, and name the roots.
  expect(screen.getByText(L.scanEmptyNone)).toBeTruthy();
  const readableRoot = "C:\\Users\\u\\AppData\\Local\\npm-cache";
  expect(screen.getByText(L.degradedUnreadableRoot(readableRoot))).toBeTruthy();
  expect(screen.getByText(L.degradedDownloadsRootMissing)).toBeTruthy();
  // Rows exist but a root was unreadable: no "nothing found" claim at all.
  cleanup();
  panel(() => Promise.resolve(result([toolCache("C:\\cache\\uv", "dev", 1_024)], { degraded })));
  await screen.findByText("C:\\cache\\uv");
  expect(screen.queryByText(L.scanEmptyNone)).toBeNull();
  expect(screen.getByText(L.scanDegradedHint)).toBeTruthy();
  // An unrecognized degraded string (older/newer backend) still surfaces raw.
  cleanup();
  panel(() => Promise.resolve(result([], { degraded: ["some-unknown-code"] })));
  await screen.findByText(L.scanDegradedHint);
  expect(screen.getByText("some-unknown-code")).toBeTruthy();
});

it("says how many rows the scan cap left out instead of implying the list is complete", async () => {
  const L = t();
  panel(() => Promise.resolve(result([toolCache("C:\\cache\\uv", "dev", 1_024)], { omitted: 137 })));
  await screen.findByText("C:\\cache\\uv");
  expect(screen.getByText(L.scanOmitted(137))).toBeTruthy();
  // A complete scan says nothing about omission.
  cleanup();
  panel(() => Promise.resolve(result([toolCache("C:\\cache\\uv", "dev", 1_024)])));
  await screen.findByText("C:\\cache\\uv");
  expect(screen.queryByText(L.scanOmitted(137))).toBeNull();
});

it("shows a capped size as a floor (≥ N), never as an exact number", async () => {
  // PERF-A: walks that stop on a bound report a verified lower bound; the
  // display must not dress it up as exact.
  panel(() => Promise.resolve(result([
    cappedToolCache("C:\\cache\\uv", 256),
    toolCache("C:\\cache\\cargo", "dev", 512),
  ])));
  await screen.findByText("C:\\cache\\uv");
  // The size rides on the same line as the localized reason, so match the
  // substring inside that row line.
  expect(screen.getByText(`· ≥ ${formatSize(256)}`, { exact: false })).toBeTruthy();
  expect(screen.getByText(`· ${formatSize(512)}`, { exact: false })).toBeTruthy();
});

it("keeps the per-item outcome on screen with a locate button and a retry", async () => {
  const L = t();
  const items = [toolCache("C:\\cache\\uv", "dev", 2_097_152), toolCache("C:\\cache\\cargo", "dev", 1_048_576)];
  vi.mocked(api.fullCleanup).mockResolvedValue(report({
    deleted: 1, failed: 1,
    item_details: [
      { path: "C:\\cache\\uv", kind: "dir", status: "deleted", message: "ok" },
      { path: "C:\\cache\\cargo", kind: "dir", status: "failed", message: "os error 32: being used by another process" },
    ],
  }));
  panel(() => Promise.resolve(result(items)));
  await screen.findByText("C:\\cache\\uv");
  fireEvent.click(screen.getByRole("button", { name: L.orphanSelectAll }));
  fireEvent.click(screen.getByRole("button", { name: new RegExp(L.cleanup) }));

  // The toast is transient; the failing row, its reason and its actions stay.
  const details = await screen.findByText(L.cleanupResultTitle);
  expect(details).toBeTruthy();
  expect(screen.getAllByText("C:\\cache\\cargo").length).toBeGreaterThan(0);
  // The failing row's reason is localized, never the raw backend fallback.
  expect(screen.getByText(L.reportFileBusy)).toBeTruthy();
  const locate = screen.getByRole("button", { name: `${L.openLocation}: C:\\cache\\cargo` });
  fireEvent.click(locate);
  await waitFor(() => expect(api.openPath).toHaveBeenCalledWith("C:\\cache\\cargo"));

  // Retry re-submits exactly the failed path.
  const retry = screen.getByRole("button", { name: `${L.retryFailedItems} (1)` });
  fireEvent.click(retry);
  await waitFor(() => expect(api.fullCleanup).toHaveBeenCalledTimes(2));
  const second = vi.mocked(api.fullCleanup).mock.calls[1]!;
  expect(second[1].map(it => it.path)).toEqual(["C:\\cache\\cargo"]);
});

// the retry path used to push an unconditional success toast on the same
// channel as the error toast. `toast.push` cancels every earlier toast on a
// channel, so a retry that failed again had its error destroyed and replaced
// with "已重试 N 项".
it("does not replace a retry failure with a success toast", async () => {
  const L = t();
  const items = [toolCache("C:\\cache\\uv", "dev", 2_097_152), toolCache("C:\\cache\\cargo", "dev", 1_048_576)];
  vi.mocked(api.fullCleanup).mockResolvedValue(report({
    deleted: 1, failed: 1,
    item_details: [
      { path: "C:\\cache\\uv", kind: "dir", status: "deleted", message: "ok" },
      { path: "C:\\cache\\cargo", kind: "dir", status: "failed", message: "os error 32: being used by another process" },
    ],
  }));
  panel(() => Promise.resolve(result(items)));
  await screen.findByText("C:\\cache\\uv");
  fireEvent.click(screen.getByRole("button", { name: L.orphanSelectAll }));
  fireEvent.click(screen.getByRole("button", { name: new RegExp(L.cleanup) }));
  await screen.findByText(L.cleanupResultTitle);

  // The retry fails the same way.
  vi.mocked(api.fullCleanup).mockResolvedValue(report({
    deleted: 0, failed: 1,
    item_details: [{ path: "C:\\cache\\cargo", kind: "dir", status: "failed", message: "os error 32: being used by another process" }],
  }));
  vi.mocked(toast.success).mockClear();
  vi.mocked(toast.error).mockClear();
  fireEvent.click(screen.getByRole("button", { name: `${L.retryFailedItems} (1)` }));
  await waitFor(() => expect(api.fullCleanup).toHaveBeenCalledTimes(2));
  await waitFor(() => expect(toast.error).toHaveBeenCalled());
  expect(toast.success, "a failed retry must not be reported as a success").not.toHaveBeenCalled();
});

it("still reports a retry that deleted something", async () => {
  const L = t();
  const items = [toolCache("C:\\cache\\uv", "dev", 2_097_152), toolCache("C:\\cache\\cargo", "dev", 1_048_576)];
  vi.mocked(api.fullCleanup).mockResolvedValueOnce(report({
    deleted: 1, failed: 1,
    item_details: [
      { path: "C:\\cache\\uv", kind: "dir", status: "deleted", message: "ok" },
      { path: "C:\\cache\\cargo", kind: "dir", status: "failed", message: "os error 32: being used by another process" },
    ],
  })).mockResolvedValueOnce(report({
    deleted: 1, failed: 0,
    item_details: [{ path: "C:\\cache\\cargo", kind: "dir", status: "deleted", message: "ok" }],
  }));
  panel(() => Promise.resolve(result(items)));
  await screen.findByText("C:\\cache\\uv");
  fireEvent.click(screen.getByRole("button", { name: L.orphanSelectAll }));
  fireEvent.click(screen.getByRole("button", { name: new RegExp(L.cleanup) }));
  await screen.findByText(L.cleanupResultTitle);
  vi.mocked(toast.success).mockClear();
  fireEvent.click(screen.getByRole("button", { name: `${L.retryFailedItems} (1)` }));
  await waitFor(() => expect(toast.success).toHaveBeenCalledWith(L.cleanupRetriedDone(1), expect.anything()));
});
it("states all four cleanup outcomes, not just deleted and failed", async () => {
  const L = t();
  vi.mocked(api.fullCleanup).mockResolvedValue(report({
    deleted: 17, failed: 3, skipped: 4, delayed: 2,
    item_details: [{ path: "C:\\cache\\uv", kind: "dir", status: "deleted", message: "ok" }],
  }));
  panel(() => Promise.resolve(result([toolCache("C:\\cache\\uv", "dev", 2_097_152)])));
  await screen.findByText("C:\\cache\\uv");
  fireEvent.click(screen.getByRole("button", { name: new RegExp(L.cleanup) }));
  await waitFor(() => expect(toast.error).toHaveBeenCalled());
  expect(vi.mocked(toast.error).mock.calls[0]![0]).toBe(L.cleanupOutcome(17, 3, 4, 2));
});

it("names the suspected / shared-root category in the cleanup confirmation", async () => {
  const L = t();
  const { requestConfirmEx } = await import("../lib/confirm");
  panel(() => Promise.resolve(result([sharedChild("C:\\cache\\npm-child", "dev", 1_024)])));
  await screen.findByText("C:\\cache\\npm-child");
  fireEvent.click(screen.getByRole("button", { name: L.orphanSelectAll }));
  fireEvent.click(screen.getByRole("button", { name: new RegExp(L.cleanup) }));
  await waitFor(() => expect(requestConfirmEx).toHaveBeenCalled());
  const message = vi.mocked(requestConfirmEx).mock.calls[0]![0].message ?? "";
  // The item count appears exactly once — no double counting.
  const countMatches = message.match(new RegExp(L.cleanupConfirmOptionalBackup(1, false).replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "g")) ?? [];
  expect(countMatches.length).toBe(1);
  expect(message).toContain(L.riskBitSuspected);
  expect(message).not.toContain(`${L.riskTierPrefix(L.riskTierLow)} ${1}`);
});

// the updater rows sit under a panel that says "not app leftovers", yet
// their reason line claimed "possible leftover of the software".
it("does not call an updater cache a suspected app leftover", async () => {
  const L = t();
  installerPanel(() => Promise.resolve(result([updaterDir("C:\\ProgramData\\Acme\\Downloads", 1_024)])));
  await screen.findByText("C:\\ProgramData\\Acme\\Downloads");
  expect(screen.getByText(L.reasonUpdaterCache, { exact: false })).toBeTruthy();
  expect(screen.queryByText(L.reasonSuspect, { exact: false })).toBeNull();
});

// "linked to this app's install/config" names an app the panel invented.
it("describes a confirmed tool cache by its bucket, not as app-linked", async () => {
  const L = t();
  panel(() => Promise.resolve(result([toolCache("C:\\Users\\u\\AppData\\Local\\uv\\cache", "dev", 1_024)])));
  await screen.findByText("C:\\Users\\u\\AppData\\Local\\uv\\cache");
  // The group header carries the same bucket label, so the row's reason line is
  // one of (at least) two occurrences.
  expect(screen.getAllByText(L.toolcacheDomainDev, { exact: false }).length).toBeGreaterThan(0);
  expect(screen.queryByText(L.reasonBelongs, { exact: false })).toBeNull();
});

// both installer groups used to carry the panel title as their header.
it("gives the two installer groups distinguishable headers", async () => {
  const L = t();
  const { container } = installerPanel(() => Promise.resolve(result([
    updaterDir("C:\\ProgramData\\Acme\\Downloads", 1_024),
    installerFile("C:\\Users\\u\\Downloads\\Acme.msi", 2_048),
  ])));
  await screen.findByText("C:\\ProgramData\\Acme\\Downloads");
  const headers = [...container.querySelectorAll('[role="presentation"]')].map(el => el.textContent ?? "");
  expect(headers.some(h => h.includes(L.scanEvidenceLabel("updater_cache", "")))).toBe(true);
  expect(headers.some(h => h.includes(L.scanEvidenceLabel("installer_package", "")))).toBe(true);
});

// "select all" must not select the keep class (user library here).
it("select-all leaves keep-class rows unchecked", async () => {
  const L = t();
  const highRisk = { ...updaterDir("C:\\ProgramData\\Acme\\Downloads", 2_048), risk: "high" as const };
  panel(() => Promise.resolve(result([
    toolCache("C:\\cache\\uv", "dev", 1_024),
    highRisk,
  ])));
  await screen.findByText("C:\\ProgramData\\Acme\\Downloads");
  fireEvent.click(screen.getByRole("button", { name: L.orphanSelectAll }));
  expect(screen.getByRole("button", { name: new RegExp(L.cleanup) }).textContent).toContain("1");
});

// "biggest first" also holds between groups.
it("orders bucket groups by their largest item", async () => {
  const tinyA = toolCache("C:\\cache\\a-tiny", "browser", 1_024);
  const hugeB = toolCache("C:\\cache\\b-huge", "dev", 4_194_304);
  const { container } = panel(() => Promise.resolve(result([tinyA, hugeB])));
  await screen.findByText("C:\\cache\\b-huge");
  const headers = [...container.querySelectorAll('[role="presentation"]')].map(el => el.textContent ?? "");
  expect(headers[0]).toContain(backendText("dev", "scan"));
});

// the toast expires; the finding stays in the header.
it("keeps the last scan count after the toast is gone", async () => {
  const L = t();
  panel(() => Promise.resolve(result([toolCache("C:\\cache\\uv", "dev", 1_024)])));
  await screen.findByText("C:\\cache\\uv");
  expect(screen.getByText(`${L.orphanJustScanned} · 1`)).toBeTruthy();
});

// a degraded scan must not announce a complete result.
it("does not claim a complete scan when a root was unreadable", async () => {
  const L = t();
  panel(() => Promise.resolve(result([], { degraded: ["downloads_root_missing"] })));
  await waitFor(() => expect(toast.info).toHaveBeenCalledWith(
    L.scanCompleteIncomplete(0),
    expect.anything(),
  ));
});
