/**
 * Remova design tokens — dark professional tool.
 *
 * SUBJECT  deep uninstall utility; job: precision + trust
 * COLOR    slate-black canvas, sky accent, hairline borders
 * TYPE     Segoe UI for chrome; Cascadia/Consolas for data columns
 * SIGNATURE 2px sky rail on selected row + status pill in header
 */

export type Theme = "light" | "dark";

export function loadTheme(): Theme {
  const v2 = localStorage.getItem("remova_theme_v2");
  if (v2 === "light" || v2 === "dark") return v2;
  // Default light — cleaner for dense tables.
  return "light";
}

export function applyTheme(theme: Theme) {
  const root = document.documentElement;
  root.dataset.theme = theme;
  if (theme === "dark") {
    root.style.setProperty("--bg", "#0B0F14");
    root.style.setProperty("--fg", "#E6EDF3");
    root.style.setProperty("--muted", "#8B9AAB");
    root.style.setProperty("--surface", "#12181F");
    root.style.setProperty("--surface-2", "#1A222C");
    root.style.setProperty("--border", "#26303D");
    root.style.setProperty("--border-strong", "#3A4A5C");
    root.style.setProperty("--accent", "#38BDF8");
    root.style.setProperty("--accent-ink", "#04121C");
    root.style.setProperty("--accent-soft", "rgba(56,189,248,.12)");
    root.style.setProperty("--danger", "#F87171");
    root.style.setProperty("--danger-soft", "rgba(248,113,113,.12)");
    root.style.setProperty("--warn", "#FBBF24");
    root.style.setProperty("--ok", "#34D399");
    root.style.setProperty("--th-bg", "#0F141A");
    root.style.setProperty("--row-hover", "#161E28");
    root.style.setProperty("--row-selected", "#0F2432");
    root.style.setProperty("--shadow", "0 1px 0 rgba(255,255,255,.03) inset, 0 8px 24px rgba(0,0,0,.45)");
    root.style.setProperty("--mono", "'Cascadia Code', 'SF Mono', Consolas, 'Courier New', monospace");
  } else {
    root.style.setProperty("--bg", "#F4F6F8");
    root.style.setProperty("--fg", "#0F172A");
    root.style.setProperty("--muted", "#5B6B7F");
    root.style.setProperty("--surface", "#FFFFFF");
    root.style.setProperty("--surface-2", "#F1F5F9");
    root.style.setProperty("--border", "#D8E0EA");
    root.style.setProperty("--border-strong", "#B0BEC9");
    root.style.setProperty("--accent", "#0284C7");
    root.style.setProperty("--accent-ink", "#FFFFFF");
    root.style.setProperty("--accent-soft", "rgba(2,132,199,.10)");
    root.style.setProperty("--danger", "#DC2626");
    root.style.setProperty("--danger-soft", "rgba(220,38,38,.08)");
    root.style.setProperty("--warn", "#D97706");
    root.style.setProperty("--ok", "#059669");
    root.style.setProperty("--th-bg", "#EEF2F6");
    root.style.setProperty("--row-hover", "#F0F7FC");
    root.style.setProperty("--row-selected", "#E0F2FE");
    root.style.setProperty("--shadow", "0 1px 2px rgba(15,23,42,.04), 0 4px 16px rgba(15,23,42,.06)");
    root.style.setProperty("--mono", "'Cascadia Code', 'SF Mono', Consolas, 'Courier New', monospace");
  }
  localStorage.setItem("remova_theme_v2", theme);
  localStorage.removeItem("remova_theme");
}
