import { describe, expect, it, vi } from "vitest";
import { ruleParseFilter } from "../components/CopilotPanel";
import type { InstalledApp } from "../types";

const apps: InstalledApp[] = [
  {
    name: "Adobe Acrobat",
    version: "1",
    publisher: "Adobe Systems",
    install_location: "C:\\Program Files\\Adobe",
    uninstall_string: "x",
    quiet_uninstall_string: "",
    source: "HKLM64",
    registry_key: "HKLM\\x",
    estimated_size_kb: 3 * 1024 * 1024,
    install_date: "20250101",
    display_icon: "",
  },
  {
    name: "TinyTool",
    version: "1",
    publisher: "Other",
    install_location: "",
    uninstall_string: "x",
    quiet_uninstall_string: "",
    source: "HKCU",
    registry_key: "HKCU\\x",
    estimated_size_kb: 1024,
    install_date: "20260101",
    display_icon: "",
  },
];

describe("ruleParseFilter", () => {
  it("filters recent installs by date and rejects unavailable conditions", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-10T12:00:00"));
    try {
      const result = ruleParseFilter(apps, "最近安装的软件");
      expect(result.list.map(a => a.name)).toEqual(["TinyTool"]);
      expect(result.intent.filter.installed_after).toBe("20251211");
      for (const query of ["可能是残留的软件", "apps with startup items", "unrecognized condition"])
        expect(() => ruleParseFilter(apps, query)).toThrow();
    } finally { vi.useRealTimers(); }
  });
  it("filters by brand name", () => {
    const { list, intent } = ruleParseFilter(apps, "找出 Adobe 相关软件");
    expect(list.map((a) => a.name)).toContain("Adobe Acrobat");
    expect(list).toHaveLength(1);
    expect(intent.filter.name_like).toBe("adobe");
  });

  it("filters by size in GB", () => {
    const { list } = ruleParseFilter(apps, "超过 2GB 的软件");
    expect(list).toHaveLength(1);
    expect(list[0]?.name).toBe("Adobe Acrobat");
  });

  it("detects analyze action", () => {
    const { intent } = ruleParseFilter(apps, "分析 Adobe 残留");
    expect(intent.action).toBe("analyze");
    expect(intent.include_leftovers).toBe(true);
  });
});
