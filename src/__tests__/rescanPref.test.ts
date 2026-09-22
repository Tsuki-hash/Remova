// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import {
  loadRescanAfterUninstall,
  saveRescanAfterUninstall,
} from "../lib/rescanPref";

describe("rescanPref", () => {
  it("defaults on and can be toggled off", () => {
    localStorage.removeItem("remova_rescan_after_uninstall");
    expect(loadRescanAfterUninstall()).toBe(true);
    saveRescanAfterUninstall(false);
    expect(loadRescanAfterUninstall()).toBe(false);
    saveRescanAfterUninstall(true);
    expect(loadRescanAfterUninstall()).toBe(true);
  });
});
