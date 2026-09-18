import { useState } from "react";
import type { UpdateInfo } from "../lib/updateCheck";
import type { CloseMode } from "../lib/closeMode";
import type { NavId, Theme } from "../lib/theme";
import type { UninstallStage } from "../components/UninstallStageBar";
import { loadCloseMode } from "../lib/closeMode";
import { loadNav, loadTheme } from "../lib/theme";

/** Shell / nav / theme chrome state extracted from App (A-1). */
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
  };
}
