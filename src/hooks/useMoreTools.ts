import { useCallback, useState } from "react";
import { toast } from "../lib/toast";
import { t } from "../i18n";
import type { InstalledApp } from "../types";
import type { ToolId } from "../components/MoreToolCard";
import { loadRescanAfterUninstall, saveRescanAfterUninstall } from "../lib/rescanPref";
import { loadRestorePointPref, saveRestorePointPref } from "../lib/restorePointPref";

/** More-page tool chrome: open panel + prefs + selection gate. */
export function useMoreTools(opts: {
  selected: InstalledApp | null;
  onGoSoftware: () => void;
}) {
  const { selected, onGoSoftware } = opts;
  const [openTool, setOpenTool] = useState<ToolId | null>(null);
  const [rescanOn, setRescanOn] = useState(() => loadRescanAfterUninstall());
  const [restorePointOn, setRestorePointOn] = useState(() => loadRestorePointPref());

  const requireSelection = useCallback(
    (run: () => void) => {
      const L = t();
      if (!selected) {
        toast.info(L.moreSelectedNeedHint || L.selectRowHint);
        onGoSoftware();
        return;
      }
      run();
    },
    [selected, onGoSoftware],
  );

  const toggleTool = useCallback((id: ToolId) => {
    setOpenTool((cur) => (cur === id ? null : id));
  }, []);

  const toggleRescan = useCallback(() => {
    setRescanOn((v) => {
      const next = !v;
      saveRescanAfterUninstall(next);
      return next;
    });
  }, []);

  const toggleRestorePoint = useCallback(() => {
    setRestorePointOn((v) => {
      const next = !v;
      saveRestorePointPref(next);
      return next;
    });
  }, []);

  return {
    openTool,
    setOpenTool,
    rescanOn,
    restorePointOn,
    setRescanOn,
    requireSelection,
    toggleTool,
    toggleRescan,
    toggleRestorePoint,
  };
}
