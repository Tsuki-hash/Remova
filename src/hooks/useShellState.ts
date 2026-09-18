import { useCallback, useMemo, useReducer, useRef } from "react";
import type { CloseMode } from "../lib/closeMode";
import type { NavId, Theme } from "../lib/theme";
import type { UninstallStage } from "../components/UninstallStageBar";
import type { UpdateInfo } from "../lib/updateCheck";
import { saveCloseMode } from "../lib/closeMode";
import { saveNav } from "../lib/theme";
import { currentLang, setLang } from "../i18n";
import { initialShellState, shellReducer } from "./reducers/shell";

/** Shell chrome state + actions (domain reducer). */
export function useShellState() {
  const [state, dispatch] = useReducer(shellReducer, undefined, initialShellState);
  const stateRef = useRef(state);
  stateRef.current = state;

  const toggleTheme = useCallback(() => dispatch({ type: "theme/toggle" }), []);
  const toggleLang = useCallback(() => {
    setLang(currentLang() === "zh" ? "en" : "zh");
    dispatch({ type: "lang/bump" });
  }, []);
  const goNav = useCallback((n: NavId) => {
    saveNav(n);
    dispatch({ type: "nav/set", nav: n });
  }, []);
  const persistCloseMode = useCallback((m: CloseMode) => {
    saveCloseMode(m);
    dispatch({ type: "closeMode/set", value: m });
  }, []);
  const toggleDetail = useCallback(() => dispatch({ type: "detail/toggle" }), []);
  const openCheckup = useCallback(() => dispatch({ type: "checkup/open" }), []);
  const closeCheckup = useCallback(() => dispatch({ type: "checkup/close" }), []);

  /** Real functional updaters (C-01) — not toggle-on-fn. */
  const setTheme = useCallback((t: Theme | ((th: Theme) => Theme)) => {
    const next = typeof t === "function" ? t(stateRef.current.theme) : t;
    if (next !== stateRef.current.theme) {
      dispatch({ type: "theme/set", theme: next });
    }
  }, []);
  const setNav = useCallback((n: NavId) => dispatch({ type: "nav/set", nav: n }), []);
  const setLangVer = useCallback((updater: number | ((v: number) => number)) => {
    const next = typeof updater === "function" ? updater(stateRef.current.langVer) : updater;
    if (next !== stateRef.current.langVer) {
      dispatch({ type: "lang/set", value: next });
    }
  }, []);
  const setShellMenu = useCallback((v: boolean) => {
    dispatch({ type: "shellMenu/set", value: v });
  }, []);
  const setCloseModeState = useCallback((m: CloseMode | null) => {
    dispatch({ type: "closeMode/set", value: m });
  }, []);
  const setUpdateInfo = useCallback((v: UpdateInfo | null) => {
    dispatch({ type: "update/set", value: v });
  }, []);
  const setCheckupOpen = useCallback((v: boolean) => {
    dispatch({ type: v ? "checkup/open" : "checkup/close" });
  }, []);
  const setCheckupOrphanCount = useCallback((v: number | null) => {
    dispatch({ type: "checkup/orphanCount", value: v });
  }, []);
  const setUninstallStage = useCallback((v: UninstallStage) => {
    dispatch({ type: "uninstallStage/set", value: v });
  }, []);
  const setShowDetail = useCallback((v: boolean | ((s: boolean) => boolean)) => {
    const next = typeof v === "function" ? v(stateRef.current.showDetail) : v;
    if (next !== stateRef.current.showDetail) {
      dispatch({ type: "detail/set", value: next });
    }
  }, []);

  const actions = useMemo(
    () => ({
      toggleTheme,
      toggleLang,
      goNav,
      persistCloseMode,
      toggleDetail,
      openCheckup,
      closeCheckup,
    }),
    [toggleTheme, toggleLang, goNav, persistCloseMode, toggleDetail, openCheckup, closeCheckup],
  );

  return {
    theme: state.theme,
    setTheme,
    nav: state.nav,
    setNav,
    langVer: state.langVer,
    setLangVer,
    shellMenu: state.shellMenu,
    setShellMenu,
    closeMode: state.closeMode,
    setCloseModeState,
    updateInfo: state.updateInfo,
    setUpdateInfo,
    checkupOpen: state.checkupOpen,
    setCheckupOpen,
    checkupOrphanCount: state.checkupOrphanCount,
    setCheckupOrphanCount,
    uninstallStage: state.uninstallStage,
    setUninstallStage,
    showDetail: state.showDetail,
    setShowDetail,
    actions,
  };
}
