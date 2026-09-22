import { describe, expect, it } from "vitest";
import { navSubtitle, navTitle } from "../lib/appNav";
import { dict as zh } from "../i18n/zh";
import type { NavId } from "../lib/theme";

const allNav: NavId[] = ["software", "startup", "services", "tasks", "orphans", "more"];

describe("navTitle", () => {
  it("maps every nav id to a non-empty dictionary label", () => {
    for (const id of allNav) {
      const title = navTitle(id, zh);
      expect(title.length).toBeGreaterThan(0);
    }
  });

  it("uses toolbox title for the more page (shell chrome, not navMore)", () => {
    expect(navTitle("more", zh)).toBe(zh.toolboxTitle);
    expect(navTitle("software", zh)).toBe(zh.navSoftware);
  });
});

describe("navSubtitle", () => {
  it("only the software list shows the installed count", () => {
    expect(navSubtitle("software", zh, 7)).toBe(zh.installedCount(7));
    expect(navSubtitle("startup", zh, 7)).toBeUndefined();
  });
});
