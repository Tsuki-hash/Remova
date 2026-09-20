import { describe, expect, it } from "vitest";
import {
  appHealth,
  bucketItem,
  cleanupProgress,
  decisionChips,
  defaultSelectable,
  groupByOrigin,
  isKeepItem,
  isRecommendedCleanup,
  isRecentInstall,
  isSuggestItem,
  leftoverReasonLine,
  originLabel,
  parseInstallDate,
  recommendScore,
  summarizeLeftovers,
} from "../lib/decision";
import type { CleanupItem, InstalledApp } from "../types";

const L = {
  chipLarge: "大体积",
  chipRecent: "最近安装",
  chipStore: "商店应用",
  chipDesktop: "桌面程序",
  chipNoUninstall: "无卸载命令",
  chipHasPath: "已定位目录",
  reasonBelongs: "与该软件安装/配置相关",
  reasonShared: "可能是共享组件，默认保留",
  reasonHigh: "高风险，请确认后再决定",
  reasonSuspect: "疑似残留，建议确认后清理",
};

function app(partial: Partial<InstalledApp> = {}): InstalledApp {
  return {
    name: "Demo",
    version: "1.0",
    publisher: "Acme",
    install_location: "C:\\Program Files\\Demo",
    uninstall_string: "msiexec /x {GUID}",
    quiet_uninstall_string: "",
    source: "HKLM64",
    registry_key: "Demo",
    estimated_size_kb: 1024,
    install_date: "",
    display_icon: "",
    ...partial,
  };
}

function item(partial: Partial<CleanupItem> = {}): CleanupItem {
  return {
    path: "C:\\x",
    kind: "dir",
    score: 50,
    confidence: "confirmed",
    risk: "low",
    reason: "r",
    evidence: [],
    ...partial,
  };
}

describe("decisionChips compact", () => {
  it("keeps at most one high-signal chip on list rows", () => {
    const chips = decisionChips(
      app({
        estimated_size_kb: 3 * 1024 * 1024,
        install_date: "20260101",
        source: "Store",
      }),
      3 * 1024 * 1024,
      { ...L, chipRecommend: "推荐清理" },
      { compact: true, now: Date.now() },
    );
    expect(chips.length).toBeLessThanOrEqual(1);
  });

  it("prefers no-uninstall warning when command is missing", () => {
    const chips = decisionChips(
      app({ uninstall_string: "", quiet_uninstall_string: "" }),
      3 * 1024 * 1024,
      { ...L, chipRecommend: "推荐清理" },
      { compact: true },
    );
    expect(chips[0]?.id).toBe("no-uninstall");
  });
});

describe("summarizeLeftovers / defaultSelectable", () => {
  it("does not default-select keep/suggest buckets", () => {
    const list = [
      item({ path: "C:\\safe", confidence: "confirmed", risk: "low" }),
      item({ path: "C:\\shared", confidence: "confirmed", risk: "low", shared: true }),
      item({ path: "C:\\user", confidence: "suspected", risk: "medium", user_data: true }),
      item({ path: "C:\\high", confidence: "confirmed", risk: "high" }),
    ];
    const s = summarizeLeftovers(list);
    expect(s.safe).toBe(1);
    expect(s.keep).toBe(3);
    expect(list.filter(defaultSelectable).map((i) => i.path)).toEqual(["C:\\safe"]);
  });
});

describe("parseInstallDate", () => {
  it("parses yyyymmdd", () => {
    const d = parseInstallDate("20240115");
    expect(d?.getFullYear()).toBe(2024);
    expect(d?.getMonth()).toBe(0);
    expect(d?.getDate()).toBe(15);
  });
  it("rejects empty", () => {
    expect(parseInstallDate("")).toBeNull();
    expect(parseInstallDate(undefined)).toBeNull();
  });
});

describe("isRecentInstall", () => {
  it("true within window", () => {
    const now = new Date(2024, 0, 20).getTime();
    expect(isRecentInstall("20240115", 30, now)).toBe(true);
  });
  it("false when old", () => {
    const now = new Date(2024, 5, 1).getTime();
    expect(isRecentInstall("20240115", 30, now)).toBe(false);
  });
});

describe("decisionChips", () => {
  it("caps at 3 and includes size when large", () => {
    const chips = decisionChips(app({ install_date: "20240101" }), 900 * 1024, L);
    expect(chips.length).toBeLessThanOrEqual(3);
    expect(chips.some((c) => c.id === "large")).toBe(true);
  });
  it("flags missing uninstall command first", () => {
    const chips = decisionChips(app({ uninstall_string: "" }), 10, L);
    expect(chips[0]?.id).toBe("no-uninstall");
  });
  it("store chip for Store apps", () => {
    const chips = decisionChips(app({ source: "Store", uninstall_string: "" }), 10, L, {
      largeKb: 10_000_000,
      recentDays: 0,
    });
    // no-uninstall + store
    expect(chips.some((c) => c.id === "store")).toBe(true);
  });
});

describe("summarizeLeftovers", () => {
  it("buckets safe / suggest / keep", () => {
    const s = summarizeLeftovers([
      item({ path: "a" }),
      item({ path: "b", confidence: "suspected" }),
      item({ path: "c", risk: "high" }),
      item({ path: "d", shared: true }),
    ]);
    expect(s.safe).toBe(1);
    expect(s.suggest).toBe(1);
    expect(s.keep).toBe(2);
    expect(s.total).toBe(4);
    expect(bucketItem(item({ shared: true }))).toBe("keep");
  });
});

describe("leftoverReasonLine", () => {
  it("prefers shared / high over confirmed", () => {
    expect(leftoverReasonLine(item({ shared: true }), L)).toBe(L.reasonShared);
    expect(leftoverReasonLine(item({ risk: "high" }), L)).toBe(L.reasonHigh);
    expect(leftoverReasonLine(item(), L)).toBe(L.reasonBelongs);
    expect(leftoverReasonLine(item({ confidence: "suspected" }), L)).toBe(L.reasonSuspect);
  });
});

describe("groupByOrigin", () => {
  it("uses folder leaf and sorts by count", () => {
    expect(originLabel("C:\\Program Files\\Acme\\App\\file.txt")).toBe("App");
    const groups = groupByOrigin([
      item({ path: "C:\\Program Files\\Chrome" }),
      item({ path: "D:\\Portable\\Chrome", confidence: "suspected" }),
      item({ path: "C:\\Users\\x\\AppData\\Local\\Steam" }),
    ]);
    expect(groups[0].origin).toBe("Chrome");
    expect(groups[0].count).toBe(2);
    expect(groups[0].safe).toBe(1);
    expect(groups[0].suggest).toBe(1);
    expect(groups[1].origin).toBe("Steam");
  });
});

describe("defaultSelectable", () => {
  it("blocks shared, user_data, high, suspected", () => {
    expect(defaultSelectable(item())).toBe(true);
    expect(defaultSelectable(item({ shared: true }))).toBe(false);
    expect(defaultSelectable(item({ user_data: true }))).toBe(false);
    expect(defaultSelectable(item({ risk: "high" }))).toBe(false);
    expect(defaultSelectable(item({ confidence: "suspected" }))).toBe(false);
  });
});

describe("isKeepItem / isSuggestItem", () => {
  it("partitions non-default items into keep vs suggest", () => {
    expect(isKeepItem(item({ risk: "high" }))).toBe(true);
    expect(isKeepItem(item({ shared: true }))).toBe(true);
    expect(isKeepItem(item({ user_data: true }))).toBe(true);
    expect(isKeepItem(item({ confidence: "suspected" }))).toBe(false);
    expect(isSuggestItem(item({ confidence: "suspected" }))).toBe(true);
    expect(isSuggestItem(item({ risk: "high" }))).toBe(false);
    expect(isSuggestItem(item())).toBe(false);
  });
});

describe("recommendScore / health / progress", () => {
  it("recommends large + old apps with uninstall cmd", () => {
    const a = app({ install_date: "20200101" });
    expect(isRecommendedCleanup(a, 3 * 1024 * 1024, Date.now())).toBe(true);
    expect(isRecommendedCleanup(a, 10, Date.now())).toBe(false);
    expect(recommendScore(a, 3 * 1024 * 1024)).toBeGreaterThan(50);
  });
  it("health attention without uninstall cmd", () => {
    const Lh = { healthOk: "OK", healthAttention: "Review" };
    expect(appHealth(app({ uninstall_string: "" }), 10, Lh).id).toBe("attention");
    expect(appHealth(app(), 10, Lh).id).toBe("ok");
  });
  it("cleanup progress complete when no failures", () => {
    const p = cleanupProgress(8, 0, 2);
    expect(p.total).toBe(10);
    expect(p.complete).toBe(true);
    expect(cleanupProgress(8, 1, 1).complete).toBe(false);
  });
});
