/** Theme helpers — CSS variables on documentElement. */

export type Theme = "light" | "dark";

export function loadTheme(): Theme {
  const v = localStorage.getItem("remova_theme");
  return v === "dark" ? "dark" : "light";
}

export function applyTheme(theme: Theme) {
  const root = document.documentElement;
  if (theme === "dark") {
    root.style.setProperty("--bg", "#0b0f14");
    root.style.setProperty("--fg", "#e7eef7");
    root.style.setProperty("--muted", "#8b9bb0");
    root.style.setProperty("--surface", "#141a22");
    root.style.setProperty("--surface-2", "#1b2330");
    root.style.setProperty("--border", "#2a3444");
    root.style.setProperty("--accent", "#2dd4bf");
    root.style.setProperty("--accent-ink", "#042f2e");
    root.style.setProperty("--danger", "#f87171");
    root.style.setProperty("--th-bg", "#1b2330");
    root.style.setProperty("--row-hover", "#1f2937");
    root.style.setProperty("--shadow", "0 8px 24px rgba(0,0,0,.35)");
  } else {
    root.style.setProperty("--bg", "#f3f6f9");
    root.style.setProperty("--fg", "#0f172a");
    root.style.setProperty("--muted", "#5b6b7f");
    root.style.setProperty("--surface", "#ffffff");
    root.style.setProperty("--surface-2", "#f8fafc");
    root.style.setProperty("--border", "#d8e0ea");
    root.style.setProperty("--accent", "#0f766e");
    root.style.setProperty("--accent-ink", "#ffffff");
    root.style.setProperty("--danger", "#dc2626");
    root.style.setProperty("--th-bg", "#f1f5f9");
    root.style.setProperty("--row-hover", "#f0fdfa");
    root.style.setProperty("--shadow", "0 10px 30px rgba(15,23,42,.06)");
  }
  localStorage.setItem("remova_theme", theme);
}
