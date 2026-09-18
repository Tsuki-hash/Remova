import { useCallback, useState } from "react";
import { toast } from "../lib/toast";
import { t } from "../i18n";
import type { InstalledApp } from "../types";
import type { ToolId } from "../components/MoreToolCard";
import { loadRescanAfterUninstall, saveRescanAfterUninstall } from "../lib/rescanPref";

/** More-page tool chrome: open panel + rescan pref + selection gate. */
export function useMoreTools(opts: {
  selected: InstalledApp | null;
  onGoSoftware: () => void;
}) {
  const { selected, onGoSoftware } = opts;
  const [openTool, setOpenTool] = useState<ToolId | null>(null);
  const [rescanOn, setRescanOn] = useState(() => loadRescanAfterUninstall());

  const requireSelection = useCallback(
    (run: () => void) => {
      const L = t();
      if (!selected) {
        toast.info(L.selectRowHint);
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

  return {
    openTool,
    setOpenTool,
    rescanOn,
    setRescanOn,
    requireSelection,
    toggleTool,
    toggleRescan,
  };
}
