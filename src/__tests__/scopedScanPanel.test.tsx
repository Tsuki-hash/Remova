// @vitest-environment jsdom
// UX-13: the specialty scan panel's presentation layer had zero coverage — the
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
  const status = screen.getByRole("status");
  // Both confirmed rows are preselected, so the header sums both.
  expect(status.textContent).toContain(L.orphanSelectedMeta(2, formatSize(3_145_728)));
  // An unmeasured row is a caveat, not a silent zero.
  expect(status.textContent).not.toContain(L.planUnknownSize(1));
  fireEvent.click(screen.getByRole("button", { name: L.orphanSelectAll }));
  expect(screen.getByRole("status").textContent).toContain(formatSize(3_145_728));
  expect(screen.getByRole("status").textContent).toContain(L.planUnknownSize(1));
});

it("groups rows by bucket and puts the largest measured item first in each group", async () => {
  const browser = toolCache("C:\\cache\\browser", "browser", 5_242_880);
  const tinyDev = toolCache("C:\\cache\\dev-tiny", "dev", 4_096);
  const bigDev = toolCache("C:\\cache\\dev-big", "dev", 2_097_152);
  const unmeasuredDev = sharedChild("C:\\cache\\dev-unknown", "dev", null);
  panel(() => Promise.resolve(result([tinyDev, browser, bigDev, unmeasuredDev])));
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
  // One header per bucket, labelled with the localized bucket name.
  const devHeader = backendText("dev", "scan");
  expect(rows.filter(r => r.includes(devHeader)).length).toBe(1);
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

it("distinguishes 'nothing found' from 'could not read the roots'", async () => {
  const L = t();
  const degraded = ["C:\\Users\\u\\AppData\\Local\\npm-cache", "D:\\jetbrains-cache"];
  panel(() => Promise.resolve(result([], { degraded })));
  await screen.findByText(L.scanDegradedHint);
  // Nothing was found AND the read was incomplete — say both, and name the roots.
  expect(screen.getByText(L.scanEmptyNone)).toBeTruthy();
  for (const root of degraded) expect(screen.getByText(root)).toBeTruthy();
  // Rows exist but a root was unreadable: no "nothing found" claim at all.
  cleanup();
  panel(() => Promise.resolve(result([toolCache("C:\\cache\\uv", "dev", 1_024)], { degraded })));
  await screen.findByText("C:\\cache\\uv");
  expect(screen.queryByText(L.scanEmptyNone)).toBeNull();
  expect(screen.getByText(L.scanDegradedHint)).toBeTruthy();
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
