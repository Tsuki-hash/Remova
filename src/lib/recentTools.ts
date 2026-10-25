/** Recently-opened toolbox cards ("More" page), newest first, capped at three. */
const KEY = "remova_recent_tools";
const CAP = 3;

export function loadRecentTools(): string[] {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    if (!Array.isArray(raw)) return [];
    return raw.filter((x): x is string => typeof x === "string").slice(0, CAP);
  } catch {
    return [];
  }
}

export function saveRecentTools(ids: string[]) {
  try {
    localStorage.setItem(KEY, JSON.stringify(ids.slice(0, CAP)));
  } catch {
    // Session-only recents are fine; the list just resets next launch.
  }
}
