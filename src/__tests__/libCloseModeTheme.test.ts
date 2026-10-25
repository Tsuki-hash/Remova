// @vitest-environment jsdom
// close-choice store + theme application (previously ~0% covered).
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  clearCloseMode,
  consumeQuitIntent,
  isCloseChoiceOpen,
  loadCloseMode,
  markQuitIntent,
  requestCloseChoice,
  resolveCloseAction,
  saveCloseMode,
  settleCloseChoice,
  subscribeCloseChoice,
} from "../lib/closeMode";
import { applyTheme, loadNav, loadThemePreference, resolveTheme } from "../lib/theme";

beforeEach(() => {
  localStorage.clear();
  clearCloseMode();
 // Drain any leftover dialog state between tests.
  if (isCloseChoiceOpen()) settleCloseChoice("cancel");
});

describe("closeMode", () => {
  it("persists and clears the close preference", () => {
    expect(loadCloseMode()).toBeNull();
    saveCloseMode("tray");
    expect(loadCloseMode()).toBe("tray");
    saveCloseMode("quit");
    expect(loadCloseMode()).toBe("quit");
    clearCloseMode();
    expect(loadCloseMode()).toBeNull();
  });

  it("quit intent is consumed exactly once", () => {
    expect(consumeQuitIntent()).toBe(false);
    markQuitIntent();
    expect(consumeQuitIntent()).toBe(true);
    expect(consumeQuitIntent()).toBe(false);
  });

  it("resolveCloseAction returns the saved mode without opening the dialog", async () => {
    saveCloseMode("quit");
    expect(await resolveCloseAction()).toBe("quit");
    expect(isCloseChoiceOpen()).toBe(false);
  });

  it("first-close dialog resolves once and notifies subscribers", async () => {
    let seen: string | null = null;
    const unsub = subscribeCloseChoice(() => {
      seen = isCloseChoiceOpen() ? "open" : "closed";
    });
    const pending = resolveCloseAction();
    expect(isCloseChoiceOpen()).toBe(true);
    expect(seen).toBe("open");
    settleCloseChoice("tray");
    expect(isCloseChoiceOpen()).toBe(false);
    await expect(pending).resolves.toBe("tray");
    unsub();
  });

  it("a second request cancels the first (single dialog)", async () => {
    const first = requestCloseChoice();
    const second = requestCloseChoice();
    await expect(first).resolves.toBe("cancel");
    settleCloseChoice("quit");
    await expect(second).resolves.toBe("quit");
  });
});

describe("theme preference", () => {
  afterEach(() => {
    Object.defineProperty(window, "matchMedia", { configurable: true, value: undefined });
  });

  it("defaults to system for new users and resolves light without matchMedia", () => {
    expect(loadThemePreference()).toBe("system");
    expect(resolveTheme("system")).toBe("light");
  });

  it("resolves and applies the system theme", () => {
    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      value: () => ({ matches: true }) as MediaQueryList,
    });
    expect(resolveTheme("system")).toBe("dark");
    applyTheme("system");
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(localStorage.getItem("remova_theme_v3")).toBe("system");
  });

  it("applies dark tokens to the root and persists", () => {
    applyTheme("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(document.documentElement.style.getPropertyValue("--bg")).toBe("#171717");
    expect(document.documentElement.style.getPropertyValue("--shadow-float")).toContain("rgba(0,0,0,.55)");
    expect(document.documentElement.style.getPropertyValue("--surface-elevated")).toBe("#242424");
    expect(localStorage.getItem("remova_theme_v3")).toBe("dark");
    expect(loadThemePreference()).toBe("dark");
  });

  it("treats a v2 value as an explicit choice and migrates off legacy keys", () => {
    localStorage.setItem("remova_theme_v2", "dark");
    expect(loadThemePreference()).toBe("dark");
    applyTheme("light");
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(document.documentElement.style.getPropertyValue("--bg")).toBe("#F6F6F6");
    expect(localStorage.getItem("remova_theme_v3")).toBe("light");
    expect(localStorage.getItem("remova_theme_v2")).toBeNull();
    expect(localStorage.getItem("remova_theme")).toBeNull();
  });

  it("still applies tokens and boots light when storage is disabled", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("denied");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("denied");
    });
    vi.spyOn(Storage.prototype, "removeItem").mockImplementation(() => {
      throw new Error("denied");
    });
    expect(loadThemePreference()).toBe("light");
    applyTheme("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
    vi.restoreAllMocks();
  });

  it("always boots on the software nav", () => {
    expect(loadNav()).toBe("software");
  });
});
