import type { CloseMode } from "../../lib/closeMode";
import type { NavId, Theme } from "../../lib/theme";
import type { UninstallStage } from "../../components/UninstallStageBar";
import type { UpdateInfo } from "../../lib/updateCheck";
import { loadCloseMode } from "../../lib/closeMode";
import { loadNav, loadTheme } from "../../lib/theme";

export type ShellState = {
  theme: Theme;
  nav: NavId;
  langVer: number;
  closeMode: CloseMode | null;
  updateInfo: UpdateInfo | null;
  checkupOpen: boolean;
  checkupOrphanCount: number | null;
  uninstallStage: UninstallStage;
  showDetail: boolean;
};

export type ShellAction =
  | { type: "theme/toggle" }
  | { type: "theme/set"; theme: Theme }
  | { type: "lang/bump" }
  | { type: "lang/set"; value: number }
  | { type: "nav/set"; nav: NavId }
  | { type: "closeMode/set"; value: CloseMode | null }
  | { type: "update/set"; value: UpdateInfo | null }
  | { type: "checkup/open" }
  | { type: "checkup/close" }
  | { type: "checkup/orphanCount"; value: number | null }
  | { type: "uninstallStage/set"; value: UninstallStage }
  | { type: "detail/toggle" }
  | { type: "detail/set"; value: boolean };

export function initialShellState(): ShellState {
  return {
    theme: loadTheme(),
    nav: loadNav(),
    langVer: 0,
    closeMode: loadCloseMode(),
    updateInfo: null,
    checkupOpen: false,
    checkupOrphanCount: null,
    uninstallStage: "idle",
    showDetail: true,
  };
}

export function shellReducer(state: ShellState, action: ShellAction): ShellState {
  switch (action.type) {
    case "theme/toggle":
      return { ...state, theme: state.theme === "dark" ? "light" : "dark" };
    case "theme/set":
      return { ...state, theme: action.theme };
    case "lang/bump":
      return { ...state, langVer: state.langVer + 1 };
    case "lang/set":
      return { ...state, langVer: action.value };
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
    case "uninstallStage/set":
      return { ...state, uninstallStage: action.value };
    case "detail/toggle":
      return { ...state, showDetail: !state.showDetail };
    case "detail/set":
      return { ...state, showDetail: action.value };
    default:
      return state;
  }
}
