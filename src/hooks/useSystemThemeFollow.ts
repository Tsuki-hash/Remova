import { useEffect } from "react";
import { applyTheme, type ThemePreference } from "../lib/theme";

/**
 * While the preference is "system", re-resolve and re-apply the OS theme
 * whenever it changes. An explicit light/dark choice ignores OS changes.
 */
export function useSystemThemeFollow(theme: ThemePreference) {
  useEffect(() => {
    if (theme !== "system" || typeof window.matchMedia !== "function") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => applyTheme("system");
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [theme]);
}
