/**
 * Remova design tokens — IObit-like consumer shell.
 *
 * SUBJECT  deep uninstall utility for everyday Windows users
 * COLOR    soft gray canvas, white surfaces, consumer blue accent
 * TYPE     Segoe UI / YaHei UI chrome; Cascadia for data
 * SIGNATURE left-nav active pill + card list rows
 */

export type Theme = "light" | "dark";

export type NavId = "software" | "startup" | "services" | "tasks" | "more";

export function loadTheme(): Theme {
  const v2 = localStorage.getItem("remova_theme_v2");
  if (v2 === "light" || v2 === "dark") return v2;
  return "light";
}

export function loadNav(): NavId {
  const v = localStorage.getItem("remova_nav");
  if (v === "startup" || v === "services" || v === "tasks" || v === "more" || v === "software") {
    return v;
  }
  return "software";
}

export function saveNav(nav: NavId) {
  localStorage.setItem("remova_nav", nav);
}

export function applyTheme(theme: Theme) {
  const root = document.documentElement;
  root.dataset.theme = theme;
  if (theme === "dark") {
    root.style.setProperty("--bg", "#0F141A");
    root.style.setProperty("--fg", "#E8EEF5");
    root.style.setProperty("--muted", "#8B9AAB");
    root.style.setProperty("--surface", "#161D26");
    root.style.setProperty("--surface-2", "#1C2530");
    root.style.setProperty("--border", "#2A3542");
    root.style.setProperty("--border-strong", "#3D4C5E");
    root.style.setProperty("--accent", "#3B8EE8");
    root.style.setProperty("--accent-ink", "#FFFFFF");
    root.style.setProperty("--accent-soft", "rgba(59,142,232,.16)");
    root.style.setProperty("--danger", "#F07178");
    root.style.setProperty("--danger-soft", "rgba(240,113,120,.14)");
    root.style.setProperty("--warn", "#F2C94C");
    root.style.setProperty("--ok", "#6FCF97");
    root.style.setProperty("--th-bg", "#121820");
    root.style.setProperty("--row-hover", "#1A2430");
    root.style.setProperty("--row-selected", "#152A3E");
    root.style.setProperty("--shadow", "0 8px 24px rgba(0,0,0,.35)");
    root.style.setProperty("--mono", "'Cascadia Code', 'SF Mono', Consolas, 'Courier New', monospace");
  } else {
    root.style.setProperty("--bg", "#F5F7FA");
    root.style.setProperty("--fg", "#1F2937");
    root.style.setProperty("--muted", "#6B7280");
    root.style.setProperty("--surface", "#FFFFFF");
    root.style.setProperty("--surface-2", "#EEF3F9");
    root.style.setProperty("--border", "#E5E7EB");
    root.style.setProperty("--border-strong", "#D1D5DB");
    root.style.setProperty("--accent", "#2F80ED");
    root.style.setProperty("--accent-ink", "#FFFFFF");
    root.style.setProperty("--accent-soft", "rgba(47,128,237,.12)");
    root.style.setProperty("--danger", "#EB5757");
    root.style.setProperty("--danger-soft", "rgba(235,87,87,.10)");
    root.style.setProperty("--warn", "#F2994A");
    root.style.setProperty("--ok", "#27AE60");
    root.style.setProperty("--th-bg", "#F3F6FA");
    root.style.setProperty("--row-hover", "#F4F8FC");
    root.style.setProperty("--row-selected", "#E8F1FD");
    root.style.setProperty("--shadow", "0 1px 2px rgba(16,24,40,.04), 0 8px 24px rgba(16,24,40,.06)");
    root.style.setProperty("--mono", "'Cascadia Code', 'SF Mono', Consolas, 'Courier New', monospace");
  }
  localStorage.setItem("remova_theme_v2", theme);
  localStorage.removeItem("remova_theme");
}
