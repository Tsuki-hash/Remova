// @vitest-environment jsdom
import { afterEach, describe, expect, it } from "vitest";
import { act, renderHook } from "@testing-library/react";
import { useSystemThemeFollow } from "../hooks/useSystemThemeFollow";
import { applyTheme, type ThemePreference } from "../lib/theme";

afterEach(() => {
  Object.defineProperty(window, "matchMedia", { configurable: true, value: undefined });
});

describe("useSystemThemeFollow", () => {
  it("re-resolves the system theme on OS change and detaches on explicit choice", () => {
    const state = { matches: false };
    const listeners = new Set<() => void>();
    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      value: (q: string) =>
        ({
          get matches() {
            return state.matches;
          },
          media: q,
          addEventListener: (_type: string, cb: () => void) => {
            listeners.add(cb);
          },
          removeEventListener: (_type: string, cb: () => void) => {
            listeners.delete(cb);
          },
        }) as unknown as MediaQueryList,
    });
    applyTheme("system");
    expect(document.documentElement.dataset.theme).toBe("light");

    const { rerender, unmount } = renderHook(
      ({ pref }) => useSystemThemeFollow(pref),
      { initialProps: { pref: "system" as ThemePreference } },
    );
    state.matches = true;
    act(() => {
      listeners.forEach((cb) => cb());
    });
    expect(document.documentElement.dataset.theme).toBe("dark");

    // An explicit choice stops following the OS theme.
    rerender({ pref: "dark" });
    state.matches = false;
    act(() => {
      listeners.forEach((cb) => cb());
    });
    expect(document.documentElement.dataset.theme).toBe("dark");
    unmount();
  });
});
