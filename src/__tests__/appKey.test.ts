import { describe, expect, it } from "vitest";
import { appKey, findReportApp } from "../lib/appKey";
import type { InstalledApp } from "../types";

const app = (registry_key: string): InstalledApp => ({
  name: "Same Product",
  version: "1",
  publisher: "Vendor",
  install_location: `C:\\Apps\\${registry_key}`,
  uninstall_string: "uninstall.exe",
  quiet_uninstall_string: "",
  source: "HKLM64",
  registry_key,
  estimated_size_kb: 1,
  install_date: "",
  display_icon: "",
});

describe("findReportApp", () => {
  it("uses app_key instead of display name for same-named install vectors", () => {
    const first = app("first");
    const second = app("second");
    const report = { app_name: second.name, app_key: appKey(second) };
    expect(findReportApp(report, [first, second], null)).toBe(second);
    expect(findReportApp(report, [first], second)).toBe(second);
  });

  it("falls back to name for legacy reports without app_key", () => {
    const first = app("first");
    const second = app("second");
    expect(findReportApp({ app_name: first.name }, [first, second], null)).toBe(first);
  });
});
