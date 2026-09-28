// @vitest-environment jsdom
// REV-QA-03: close-choice store + theme application (previously ~0% covered).
import { beforeEach, describe, expect, it } from "vitest";
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
import { applyTheme, loadNav, loadTheme } from "../lib/theme";

beforeEach(() => {
  localStorage.clear();
  clearCloseMode();
  // Drain any leftover dialog state between tests.
  if (isCloseChoiceOpen()) settleCloseChoice("cancel");
});

describe("closeMode (REV-QA-03)", () => {
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

describe("theme (REV-QA-03)", () => {
  it("defaults to light when nothing stored", () => {
    expect(loadTheme()).toBe("light");
  });

  it("applies dark tokens to the root and persists", () => {
    applyTheme("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(document.documentElement.style.getPropertyValue("--bg")).toBe("#171717");
    expect(localStorage.getItem("remova_theme_v2")).toBe("dark");
    expect(loadTheme()).toBe("dark");
  });

  it("applies light tokens and migrates off the legacy key", () => {
    localStorage.setItem("remova_theme", "dark");
    applyTheme("light");
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(document.documentElement.style.getPropertyValue("--bg")).toBe("#F6F6F6");
    expect(localStorage.getItem("remova_theme_v2")).toBe("light");
    expect(localStorage.getItem("remova_theme")).toBeNull();
  });

  it("always boots on the software nav", () => {
    expect(loadNav()).toBe("software");
  });
});
