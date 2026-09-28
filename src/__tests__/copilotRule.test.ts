import { describe, expect, it } from "vitest";
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
