// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { loadRestorePointPref, saveRestorePointPref } from "../lib/restorePointPref";

describe("restore point preference", () => {
  afterEach(() => {
    localStorage.clear();
    vi.restoreAllMocks();
  });

  it("defaults to enabled when nothing was saved", () => {
    expect(loadRestorePointPref()).toBe(true);
  });

  it("persists off and on", () => {
    saveRestorePointPref(false);
    expect(loadRestorePointPref()).toBe(false);
    saveRestorePointPref(true);
    expect(loadRestorePointPref()).toBe(true);
  });

  it("falls back to enabled when storage throws", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("storage blocked");
    });
    expect(loadRestorePointPref()).toBe(true);
    expect(() => saveRestorePointPref(false)).not.toThrow();
  });
});
