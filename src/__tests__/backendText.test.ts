// @vitest-environment jsdom
import { afterEach, expect, it } from "vitest";
import { setLang, t } from "../i18n";
import { backendText } from "../lib/backendText";
afterEach(() => setLang("zh"));
it.each(["zh", "en"] as const)("localizes execution, scan and idle diagnostics in %s", lang => {
  setLang(lang);
  const L = t();
  expect(backendText("uninstaller finished: vendor.exe")).toBe(L.uninstallOk);
  expect(backendText("uninstaller exited with Some(1603)")).toBe(L.backendUninstallExit("1603"));
  expect(backendText("uninstaller timed out (5m): vendor.exe")).toBe(L.backendTimeout);
  expect(backendText("backup failed for 2 item(s); aborted")).toBe(L.backendBackupFailed);
  expect(backendText("SRSetRestorePointW failed")).toBe(L.restorePointFail);
  expect(backendText("sc delete Vendor: failed or not found")).toBe(L.backendNativeDeleteFailed);
  expect(backendText("programFiles", "scan")).toBe(L.linkedProgramFiles);
  expect(backendText("Windows service leftover: Vendor", "scan")).toBe(L.backendService);
  expect(backendText("idle_size_partial", "idle")).toBe(L.idleEvSizePartial);
  expect(backendText("future_unknown: raw")).toBe(L.backendDetailUnavailable);
  expect(backendText("future_unknown", "idle")).toBe(L.backendDetailUnavailable);
  expect(backendText("future scanner reason", "scan")).toBe(L.backendDetailUnavailable);
  expect(backendText("AppData folder matching product name", "scan")).toBe(L.linkedConfigFiles);
  expect(backendText("WebView2 / Electron cache folder", "scan")).toBe(L.linkedConfigFiles);
  expect(backendText("updater cache", "scan")).toBe(L.installerTitle);
  expect(backendText("possible installer package", "scan")).toBe(L.installerMaybeTitle);
  expect(backendText("shared tool cache", "scan")).toBe(L.toolcacheSharedTitle);
  expect(backendText("tool cache (dev)", "scan")).toBe(L.toolcacheTitle);
  expect(backendText("Run startup: Vendor", "scan")).toBe(L.linkedStartup);
  expect(backendText("")).toBe("");
  for (const code of ["constructor", "toString", "__proto__"]) {
    for (const context of ["cleanup", "scan", "idle"] as const) {
      expect(backendText(code, context)).toBe(L.backendDetailUnavailable);
    }
  }
});

/**
 * Every `bucket` / `reason` literal the Rust scanners write must resolve to a
 * localized string. A missing entry used to make 100% of the rows in the
 * installer and tool-cache panels render as "no localized description yet",
 * because the panel preferred `bucket` over `reason`.
 */
it.each(["zh", "en"] as const)("every specialty-scan bucket and reason localizes in %s", lang => {
  setLang(lang);
  const L = t();
  const buckets = ["updater_cache", "download_pkg", "dev", "game", "browser"];
  for (const b of buckets) {
    expect(backendText(b, "scan"), b).not.toBe(L.backendDetailUnavailable);
  }
  const reasons = [
    "installer package",
    "possible installer package",
    "updater cache",
    "tool cache (dev)",
    "tool cache (browser)",
    "shared tool cache",
  ];
  for (const r of reasons) {
    expect(backendText(r, "scan"), r).not.toBe(L.backendDetailUnavailable);
  }
});
