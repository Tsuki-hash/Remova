import { useCallback, useMemo, useReducer } from "react";
import type { CloseMode } from "../lib/closeMode";
import type { NavId, ThemePreference } from "../lib/theme";
import type { UninstallStage } from "../components/UninstallStageBar";
import type { UpdateInfo } from "../lib/updateCheck";
import { saveCloseMode } from "../lib/closeMode";
import { currentLang, setLang } from "../i18n";
import { initialShellState, shellReducer } from "./reducers/shell";

/** Shell chrome state + actions (domain reducer). */
export function useShellState() {
  const [state, dispatch] = useReducer(shellReducer, undefined, initialShellState);

  const toggleLang = useCallback(() => {
    setLang(currentLang() === "zh" ? "en" : "zh");
    dispatch({ type: "lang/bump" });
  }, []);
  const goNav = useCallback((n: NavId) => {
 // nav is not restored on boot (always software) — no dead persist.
    dispatch({ type: "nav/set", nav: n });
  }, []);
  const persistCloseMode = useCallback((m: CloseMode) => {
    saveCloseMode(m);
    dispatch({ type: "closeMode/set", value: m });
  }, []);
  const toggleDetail = useCallback(() => dispatch({ type: "detail/toggle" }), []);
  const openCheckup = useCallback(() => dispatch({ type: "checkup/open" }), []);
  const closeCheckup = useCallback(() => dispatch({ type: "checkup/close" }), []);

  /** Functional updaters run inside the reducer, on the newest state. */
  const setTheme = useCallback((t: ThemePreference | ((th: ThemePreference) => ThemePreference)) => {
    dispatch({ type: "theme/set", theme: t });
  }, []);
  const setNav = useCallback((n: NavId) => dispatch({ type: "nav/set", nav: n }), []);
  const setLangVer = useCallback((updater: number | ((v: number) => number)) => {
    dispatch({ type: "lang/set", value: updater });
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
  const setCheckupOrphanCompleteness = useCallback(
    (v: import("../types").ScanCompleteness | null) => {
      dispatch({ type: "checkup/orphanCompleteness", value: v });
    },
    [],
  );
  const setUninstallStage = useCallback((v: UninstallStage) => {
    dispatch({ type: "uninstallStage/set", value: v });
  }, []);
  const setShowDetail = useCallback((v: boolean | ((s: boolean) => boolean)) => {
    dispatch({ type: "detail/set", value: v });
  }, []);

  const actions = useMemo(
    () => ({
      toggleLang,
      goNav,
      persistCloseMode,
      toggleDetail,
      openCheckup,
      closeCheckup,
    }),
    [toggleLang, goNav, persistCloseMode, toggleDetail, openCheckup, closeCheckup],
  );

  return useMemo(
    () => ({
      theme: state.theme,
      setTheme,
      nav: state.nav,
      setNav,
      langVer: state.langVer,
      setLangVer,
      closeMode: state.closeMode,
      setCloseModeState,
      updateInfo: state.updateInfo,
      setUpdateInfo,
      checkupOpen: state.checkupOpen,
      setCheckupOpen,
      checkupOrphanCount: state.checkupOrphanCount,
      setCheckupOrphanCount,
      checkupOrphanCompleteness: state.checkupOrphanCompleteness,
      setCheckupOrphanCompleteness,
      uninstallStage: state.uninstallStage,
      setUninstallStage,
      showDetail: state.showDetail,
      setShowDetail,
      actions,
    }),
    [
      state.theme,
      state.nav,
      state.langVer,
      state.closeMode,
      state.updateInfo,
      state.checkupOpen,
      state.checkupOrphanCount,
      state.checkupOrphanCompleteness,
      state.uninstallStage,
      state.showDetail,
      setTheme,
      setNav,
      setLangVer,
      setCloseModeState,
      setUpdateInfo,
      setCheckupOpen,
      setCheckupOrphanCount,
      setUninstallStage,
      setShowDetail,
      actions,
    ],
  );
}
