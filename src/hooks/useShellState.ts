import { useCallback, useState } from "react";
import type { UpdateInfo } from "../lib/updateCheck";
import type { CloseMode } from "../lib/closeMode";
import type { NavId, Theme } from "../lib/theme";
import type { UninstallStage } from "../components/UninstallStageBar";
import { loadCloseMode, saveCloseMode } from "../lib/closeMode";
import { loadNav, loadTheme, saveNav } from "../lib/theme";
import { currentLang, setLang } from "../i18n";

/** Shell / nav / theme chrome state + actions extracted from App (A-1). */
export function useShellState() {
  const [theme, setTheme] = useState<Theme>(loadTheme());
  const [nav, setNav] = useState<NavId>(loadNav());
  const [langVer, setLangVer] = useState(0);
  const [shellMenu, setShellMenu] = useState(false);
  const [closeMode, setCloseModeState] = useState<CloseMode | null>(() => loadCloseMode());
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [checkupOpen, setCheckupOpen] = useState(false);
  const [checkupOrphanCount, setCheckupOrphanCount] = useState<number | null>(null);
  const [uninstallStage, setUninstallStage] = useState<UninstallStage>("idle");
  const [showDetail, setShowDetail] = useState(true);

  const toggleTheme = useCallback(() => {
    setTheme((th) => (th === "dark" ? "light" : "dark"));
  }, []);

  const toggleLang = useCallback(() => {
    const next = currentLang() === "zh" ? "en" : "zh";
    setLang(next);
    setLangVer((v) => v + 1);
  }, []);

  const goNav = useCallback((n: NavId) => {
    setNav(n);
    saveNav(n);
  }, []);

  const persistCloseMode = useCallback((m: CloseMode) => {
    setCloseModeState(m);
    saveCloseMode(m);
  }, []);

  const toggleDetail = useCallback(() => setShowDetail((v) => !v), []);

  const openCheckup = useCallback(() => setCheckupOpen(true), []);
  const closeCheckup = useCallback(() => setCheckupOpen(false), []);

  return {
    theme,
    setTheme,
    nav,
    setNav,
    langVer,
    setLangVer,
    shellMenu,
    setShellMenu,
    closeMode,
    setCloseModeState,
    updateInfo,
    setUpdateInfo,
    checkupOpen,
    setCheckupOpen,
    checkupOrphanCount,
    setCheckupOrphanCount,
    uninstallStage,
    setUninstallStage,
    showDetail,
    setShowDetail,
    actions: {
      toggleTheme,
      toggleLang,
      goNav,
      persistCloseMode,
      toggleDetail,
      openCheckup,
      closeCheckup,
    },
  };
}
