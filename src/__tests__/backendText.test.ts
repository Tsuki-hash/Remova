// @vitest-environment jsdom
import { afterEach, describe, expect, it } from "vitest";
import { setLang, t } from "../i18n";
import { backendErrorText, backendText } from "../lib/backendText";
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
  expect(backendText("uninstaller finished (reboot required): vendor.exe")).toBe(L.backendRebootRequired);
  expect(backendText("official uninstaller initiated reboot; residual cleanup skipped")).toBe(L.backendRebootInitiated);
  expect(backendText("system reboot pending; residual cleanup postponed")).toBe(L.backendRebootInitiated);
  expect(backendText("operation failed: process image changed; refresh")).toBe(L.lockImageChanged);
  expect(backendText("operation failed: process no longer holds this path")).toBe(L.lockNoLongerHolds);
  expect(backendText("operation failed: path is not from the latest cleanup")).toBe(L.lockTargetStale);
  expect(backendText("elevated HKCU analysis refused")).toBe(L.elevatedHkcuBlocked);
  expect(backendText("app is not from the latest scan")).toBe(L.backendAppStale);
  expect(backendText("analysis must run before cleanup")).toBe(L.backendAnalysisRequired);
  expect(backendText("uninstall command uses a relative executable; refused")).toBe(
    L.uninstallRelativeRefused,
  );
  expect(backendText("cleanup item not from the latest analysis: D:\\Data\\X")).toBe(
    L.backendCleanupItemStale("D:\\Data\\X"),
  );
  expect(backendErrorText("future_unknown")).toBe("future_unknown");
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

describe("1.4.1 backend error strings", () => {
  it("localizes the migration and regenerable error strings", () => {
    expect(backendErrorText("migrate:cancelled")).toBe(t().migrateCancelled);
    expect(backendErrorText("cleanup cancelled during backup")).toBe(t().backendCleanupCancelledBackup);
    expect(backendErrorText("not a regenerable layout: C:/Windows/Temp")).toBe(
      t().backendNotRegenerable("C:/Windows/Temp"),
    );
    expect(
      backendErrorText("migration is not in the cutover state (current: finished)"),
    ).toBe(t().backendMigrateState);
    expect(backendErrorText("backup failed; npm packages kept: seal boom")).toBe(
      t().backendNpmBackupFailed,
    );
    expect(
      backendErrorText("package is no longer listed by the fresh scan: rimraf"),
    ).toBe(t().backendNpmStale("rimraf"));
    expect(backendErrorText("npm global folder not found")).toBe(t().backendNpmNoRoot);
    expect(
      backendErrorText("not an admitted npm global package: ghost"),
    ).toBe(t().backendNpmNotAdmitted("ghost"));
    // Unknown strings keep the verbatim passthrough contract.
    expect(backendErrorText("some future backend string")).toBe("some future backend string");
  });
});
