/**
 * Remova design tokens — modern Windows tool (not SaaS dashboard).
 *
 * SUBJECT  deep uninstall utility
 * COLOR    90% neutral gray/white, 5% brand blue for interaction, 5% status
 * TYPE     Segoe UI / YaHei UI chrome; Cascadia for data
 * SIGNATURE quiet nav, high-density list, restrained radius/shadow
 */

export type Theme = "light" | "dark";

/** The stored preference; "system" resolves against the OS theme at apply time. */
export type ThemePreference = "light" | "dark" | "system";

const V2_KEY = "remova_theme_v2";
const V3_KEY = "remova_theme_v3";
const LEGACY_KEY = "remova_theme";

export function loadThemePreference(): ThemePreference {
  try {
    const v3 = localStorage.getItem(V3_KEY);
    if (v3 === "light" || v3 === "dark" || v3 === "system") return v3;
    // v2 was written by the old two-state toggle: it is an explicit choice, not a default.
    const v2 = localStorage.getItem(V2_KEY);
    if (v2 === "light" || v2 === "dark") return v2;
  } catch {
    // Storage disabled — boot on a readable theme instead of throwing during startup.
    return "light";
  }
  return "system";
}

function systemTheme(): Theme {
  if (typeof window !== "undefined" && typeof window.matchMedia === "function") {
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }
  return "light";
}

export function resolveTheme(pref: ThemePreference): Theme {
  return pref === "system" ? systemTheme() : pref;
}

export type NavId = "software" | "startup" | "services" | "tasks" | "orphans" | "more";

/** Always open on the software list (main uninstall path); do not restore last tab. */
export function loadNav(): NavId {
  return "software";
}

export function applyTheme(pref: ThemePreference) {
  const theme = resolveTheme(pref);
  const root = document.documentElement;
  root.dataset.theme = theme;
  if (theme === "dark") {
 // Professional Windows-tool dark (not pure black dashboard)
    root.style.setProperty("--bg", "#171717");
    root.style.setProperty("--fg", "#F5F5F5");
    root.style.setProperty("--muted", "#A3A3A3");
    root.style.setProperty("--surface", "#1E1E1E");
    root.style.setProperty("--surface-2", "#242424");
    root.style.setProperty("--border", "#333333");
    root.style.setProperty("--border-strong", "#404040");
    root.style.setProperty("--accent", "#60A5FA");
    root.style.setProperty("--accent-ink", "#0B1220");
    root.style.setProperty("--accent-soft", "rgba(96,165,250,.14)");
    root.style.setProperty("--danger", "#F87171");
    root.style.setProperty("--danger-soft", "rgba(248,113,113,.12)");
    root.style.setProperty("--warn", "#FBBF24");
    root.style.setProperty("--ok", "#4ADE80");
 // Text-grade tokens: small-size text needs >=4.5:1 on light panels.
    root.style.setProperty("--warn-ink", "#FBBF24");
    root.style.setProperty("--ok-ink", "#4ADE80");
    root.style.setProperty("--accent-text", "#60A5FA");
    root.style.setProperty("--danger-text", "#F87171");
    root.style.setProperty("--th-bg", "#242424");
    root.style.setProperty("--row-hover", "#2A2A2A");
    root.style.setProperty("--row-selected", "rgba(96,165,250,.12)");
    root.style.setProperty("--shadow", "none");
    // Floating layers only (dialogs, menus, overlays): one elevation step above
    // the flat panels, heavier in dark where the canvas is already near-black.
    root.style.setProperty("--shadow-float", "0 16px 48px rgba(0,0,0,.55)");
    root.style.setProperty("--surface-elevated", "#242424");
    root.style.setProperty("--mono", "'Cascadia Code', 'SF Mono', Consolas, 'Courier New', monospace");
  } else {
 // Neutral gray canvas + white panels (docs §2 / §11)
    root.style.setProperty("--bg", "#F6F6F6");
    root.style.setProperty("--fg", "#1F1F1F");
    root.style.setProperty("--muted", "#6B6B6B");
    root.style.setProperty("--surface", "#FFFFFF");
    root.style.setProperty("--surface-2", "#FAFAFA");
    root.style.setProperty("--border", "#E5E5E5");
    root.style.setProperty("--border-strong", "#D1D5DB");
    root.style.setProperty("--accent", "#2563EB");
    root.style.setProperty("--accent-ink", "#FFFFFF");
    root.style.setProperty("--accent-soft", "rgba(37,99,235,.10)");
    root.style.setProperty("--danger", "#DC2626");
    root.style.setProperty("--danger-soft", "rgba(220,38,38,.08)");
    root.style.setProperty("--warn", "#D97706");
    root.style.setProperty("--ok", "#16A34A");
    root.style.setProperty("--warn-ink", "#B45309");
    root.style.setProperty("--ok-ink", "#15803D");
    root.style.setProperty("--accent-text", "#1D4ED8");
    root.style.setProperty("--danger-text", "#B91C1C");
    root.style.setProperty("--th-bg", "#FAFAFA");
    root.style.setProperty("--row-hover", "#F6F6F6");
    root.style.setProperty("--row-selected", "rgba(37,99,235,.08)");
    root.style.setProperty("--shadow", "none");
    root.style.setProperty("--shadow-float", "0 16px 48px rgba(0,0,0,.18)");
    root.style.setProperty("--surface-elevated", "#FFFFFF");
    root.style.setProperty("--mono", "'Cascadia Code', 'SF Mono', Consolas, 'Courier New', monospace");
  }
  try {
    localStorage.setItem(V3_KEY, pref);
    localStorage.removeItem(V2_KEY);
    localStorage.removeItem(LEGACY_KEY);
  } catch {
    // Storage unavailable — the tokens above still apply for this session.
  }
}
