import type { CloseMode } from "../../lib/closeMode";
import type { NavId, ThemePreference } from "../../lib/theme";
import type { UninstallStage } from "../../components/UninstallStageBar";
import type { UpdateInfo } from "../../lib/updateCheck";
import { loadCloseMode } from "../../lib/closeMode";
import { loadNav, loadThemePreference } from "../../lib/theme";

export type ShellState = {
  theme: ThemePreference;
  nav: NavId;
  langVer: number;
  closeMode: CloseMode | null;
  updateInfo: UpdateInfo | null;
  checkupOpen: boolean;
  checkupOrphanCount: number | null;
  /** Completeness of the scan that produced the count above. */
  checkupOrphanCompleteness: import("../../types").ScanCompleteness | null;
  uninstallStage: UninstallStage;
  showDetail: boolean;
};

export type ShellAction =
  | {
      type: "theme/set";
      theme: ThemePreference | ((th: ThemePreference) => ThemePreference);
    }
  | { type: "lang/bump" }
  | { type: "lang/set"; value: number | ((v: number) => number) }
  | { type: "nav/set"; nav: NavId }
  | { type: "closeMode/set"; value: CloseMode | null }
  | { type: "update/set"; value: UpdateInfo | null }
  | { type: "checkup/open" }
  | { type: "checkup/close" }
  | { type: "checkup/orphanCount"; value: number | null }
  | {
      type: "checkup/orphanCompleteness";
      value: import("../../types").ScanCompleteness | null;
    }
  | { type: "uninstallStage/set"; value: UninstallStage }
  | { type: "detail/toggle" }
  | { type: "detail/set"; value: boolean | ((v: boolean) => boolean) };

export function initialShellState(): ShellState {
  return {
    theme: loadThemePreference(),
    nav: loadNav(),
    langVer: 0,
    closeMode: loadCloseMode(),
    updateInfo: null,
    checkupOpen: false,
    checkupOrphanCount: null,
    checkupOrphanCompleteness: null,
    uninstallStage: "idle",
    showDetail: true,
  };
}

export function shellReducer(state: ShellState, action: ShellAction): ShellState {
  switch (action.type) {
    case "theme/set":
      // Apply a function updater against the newest state, not a render-time
      // ref: two batched increments must both land.
      return {
        ...state,
        theme:
          typeof action.theme === "function" ? action.theme(state.theme) : action.theme,
      };
    case "lang/bump":
      return { ...state, langVer: state.langVer + 1 };
    case "lang/set":
      return {
        ...state,
        langVer:
          typeof action.value === "function" ? action.value(state.langVer) : action.value,
      };
    case "nav/set":
      return { ...state, nav: action.nav };
    case "closeMode/set":
      return { ...state, closeMode: action.value };
    case "update/set":
      return { ...state, updateInfo: action.value };
    case "checkup/open":
      return { ...state, checkupOpen: true };
    case "checkup/close":
      return { ...state, checkupOpen: false };
    case "checkup/orphanCount":
      return { ...state, checkupOrphanCount: action.value };
    case "checkup/orphanCompleteness":
      return { ...state, checkupOrphanCompleteness: action.value };
    case "uninstallStage/set":
      return { ...state, uninstallStage: action.value };
    case "detail/toggle":
      return { ...state, showDetail: !state.showDetail };
    case "detail/set":
      return {
        ...state,
        showDetail:
          typeof action.value === "function" ? action.value(state.showDetail) : action.value,
      };
    default:
      return state;
  }
}
