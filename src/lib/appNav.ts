import type { NavId } from "./theme";
import type { Strings } from "../i18n";

/** Shell title for the active nav (F-R7-03: keep chrome labels out of App.tsx). */
export function navTitle(nav: NavId, L: Strings): string {
  switch (nav) {
    case "software":
      return L.navSoftware;
    case "startup":
      return L.navStartup;
    case "services":
      return L.navServices;
    case "tasks":
      return L.navTasks;
    case "orphans":
      return L.navOrphans;
    case "more":
      return L.toolboxTitle;
  }
}

/** Count subtitle — only the software list shows installed-app totals. */
export function navSubtitle(nav: NavId, L: Strings, appCount: number): string | undefined {
  return nav === "software" ? L.installedCount(appCount) : undefined;
}
