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
  gateReasonText,
  leftoverReasonLine,
  maxRiskOf,
  originLabel,
  parseInstallDate,
  recommendScore,
  riskTierLabel,
  summarizeLeftovers,
  buildCleanupRiskBits,
  formatRiskNote,
} from "../lib/decision";
import type { CleanupItem, InstalledApp } from "../types";

// QA-12: frozen clock — install-date branches must not silently change as the
// wall clock moves (the old Date.now() assertions decayed after 2026-01-31).
const NOW = Date.UTC(2026, 6, 1);

const riskL = {
  riskBitHigh: "高风险：请逐项确认后再删除",
  riskBitUserData: "用户数据：默认不删，请确认是缓存后再清",
  riskBitUserLibrary: "用户库数据：可能含存档/个人文件",
  riskBitShared: "共享运行库：其它软件可能仍在用",
};

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
  reasonUserData: "用户数据，默认不删",
  reasonUserLibrary: "用户库中的应用数据（可能含存档），确认后再清",
  reasonIgnored: "已在忽略列表中，跳过",
  reasonPathProtected: "系统 PATH 条目，受保护",
  reasonSafetyGate: "未通过删除安全门，跳过",
  reasonNotAssociated: "与当前软件无可靠关联，跳过",
  reasonPathMissing: "路径已不存在",
  reasonNotInPath: "PATH 中已不存在该条目",
  reasonRebootDelete: "已安排重启后删除",
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
      { compact: true, now: NOW },
    );
    // 3 GB app with an uninstall cmd → exactly the recommend chip, nothing else.
    expect(chips.map((c) => c.id)).toEqual(["recommend"]);
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

describe("maxRiskOf / riskTierLabel", () => {
  const riskL = {
    riskTierSafe: "安全",
    riskTierLow: "低风险",
    riskTierMedium: "中风险",
    riskTierHigh: "高风险",
  };
  it("picks highest risk among items", () => {
    expect(maxRiskOf([item({ risk: "low" }), item({ risk: "medium" })])).toBe("medium");
    expect(maxRiskOf([item({ risk: "high" }), item({ risk: "low" })])).toBe("high");
    expect(maxRiskOf([])).toBe("low");
  });
  it("maps tier labels", () => {
    expect(riskTierLabel("high", riskL)).toBe("高风险");
    expect(riskTierLabel("medium", riskL)).toBe("中风险");
    expect(riskTierLabel("low", riskL)).toBe("低风险");
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

describe("leftoverReasonLine / gateReasonText", () => {
  it("leftoverReasonLine prefers user_data then library/shared/high/confirmed", () => {
    expect(leftoverReasonLine(item({ user_data: true }), L)).toBe(L.reasonUserData);
    expect(leftoverReasonLine(item({ user_library: true }), L)).toBe(L.reasonUserLibrary);
    expect(leftoverReasonLine(item({ shared: true }), L)).toBe(L.reasonShared);
    expect(leftoverReasonLine(item({ risk: "high" }), L)).toBe(L.reasonHigh);
    expect(leftoverReasonLine(item(), L)).toBe(L.reasonBelongs);
    expect(leftoverReasonLine(item({ confidence: "suspected" }), L)).toBe(L.reasonSuspect);
    // library paths are never default-selected (confirm bucket)
    expect(defaultSelectable(item({ user_library: true, confidence: "confirmed" }))).toBe(false);
    expect(isSuggestItem(item({ user_library: true, confidence: "confirmed" }))).toBe(true);
  });

  it("gateReasonText maps delete-gate skip codes to user copy", () => {
    expect(gateReasonText("user_data red line", L)).toBe(L.reasonUserData);
    expect(gateReasonText("shared runtime", L)).toBe(L.reasonShared);
    expect(gateReasonText("ignored path", L)).toBe(L.reasonIgnored);
    expect(gateReasonText("protected PATH entry", L)).toBe(L.reasonPathProtected);
    expect(gateReasonText("failed safety gate", L)).toBe(L.reasonSafetyGate);
    expect(gateReasonText("path not associated with app", L)).toBe(L.reasonNotAssociated);
    expect(gateReasonText("path missing", L)).toBe(L.reasonPathMissing);
    expect(gateReasonText("not found in PATH", L)).toBe(L.reasonNotInPath);
    expect(gateReasonText("reboot delete", L)).toBe(L.reasonRebootDelete);
    expect(gateReasonText("something else", L)).toBe("something else");
  });

  it("gateReasonText maps common Chinese backend copy (F-R6-09)", () => {
    expect(gateReasonText("用户数据，默认不删", L)).toBe(L.reasonUserData);
    expect(gateReasonText("共享运行库/安装缓存目录", L)).toBe(L.reasonShared);
    expect(gateReasonText("已在忽略列表中，跳过", L)).toBe(L.reasonIgnored);
    expect(gateReasonText("系统 PATH 条目，受保护", L)).toBe(L.reasonPathProtected);
    expect(gateReasonText("未通过删除安全门，跳过", L)).toBe(L.reasonSafetyGate);
    expect(gateReasonText("与当前软件无可靠关联，跳过", L)).toBe(L.reasonNotAssociated);
    expect(gateReasonText("路径已不存在", L)).toBe(L.reasonPathMissing);
    expect(gateReasonText("PATH 中已不存在该条目", L)).toBe(L.reasonNotInPath);
    expect(gateReasonText("已安排重启后删除", L)).toBe(L.reasonRebootDelete);
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
    expect(groups[0]?.origin).toBe("Chrome");
    expect(groups[0]?.count).toBe(2);
    expect(groups[0]?.safe).toBe(1);
    expect(groups[0]?.suggest).toBe(1);
    expect(groups[1]?.origin).toBe("Steam");
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
    expect(isRecommendedCleanup(a, 3 * 1024 * 1024, NOW)).toBe(true);
    expect(isRecommendedCleanup(a, 10, NOW)).toBe(false);
    // 1 GB + 90 days old → recommend; recent install → not yet.
    expect(isRecommendedCleanup(app({ install_date: "20260301" }), 1024 * 1024, NOW)).toBe(true);
    expect(isRecommendedCleanup(app({ install_date: "20260601" }), 1024 * 1024, NOW)).toBe(false);
    expect(recommendScore(a, 3 * 1024 * 1024, NOW)).toBeGreaterThan(50);
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

describe("buildCleanupRiskBits / formatRiskNote", () => {
  it("covers high/user/library/shared once each", () => {
    const bits = buildCleanupRiskBits(
      [
        item({ risk: "high" }),
        item({ user_data: true }),
        item({ user_library: true }),
        item({ shared: true }),
        item({ risk: "high", shared: true }),
      ],
      riskL,
    );
    expect(bits).toEqual([
      riskL.riskBitHigh,
      riskL.riskBitUserData,
      riskL.riskBitUserLibrary,
      riskL.riskBitShared,
    ]);
    expect(buildCleanupRiskBits([item({})], riskL)).toEqual([]);
    expect(formatRiskNote([], "注意")).toBe("");
    expect(formatRiskNote(["a", "b"], "注意")).toBe("\n\n注意\na\nb");
  });
});
