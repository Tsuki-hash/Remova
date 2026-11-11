import { useCallback, useEffect, useRef, useState } from "react";
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
  initialTool?: ToolId | null;
}) {
  const { selected, onGoSoftware } = opts;
  const [openTool, setOpenTool] = useState<ToolId | null>(opts.initialTool ?? null);
  const [rescanOn, setRescanOn] = useState(() => loadRescanAfterUninstall());
  const [restorePointOn, setRestorePointOn] = useState(() => loadRestorePointPref());
  // The element that opened the panel. Closing it must put keyboard focus back
  // where the user was, or the next Tab starts from the top of the page.
  const openerRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (openTool) return;
    const opener = openerRef.current;
    if (!opener) return;
    openerRef.current = null;
    // The card may have been filtered out while the panel was open; only
    // restore focus when it is still in the document.
    if (!opener.isConnected) return;
    opener.focus({ preventScroll: true });
  }, [openTool]);

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

  const openFromTrigger = useCallback((
    id: ToolId,
    { opener = document.activeElement, toggle = false }: { opener?: Element | null; toggle?: boolean } = {},
  ) => {
    openerRef.current =
      opener instanceof HTMLElement && opener !== document.body ? opener : null;
    setOpenTool((cur) => (toggle && cur === id ? null : id));
  }, []);

  const toggleTool = useCallback((id: ToolId) => openFromTrigger(id, { toggle: true }), [openFromTrigger]);

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
    openFromTrigger,
    rescanOn,
    restorePointOn,
    setRescanOn,
    requireSelection,
    toggleTool,
    toggleRescan,
    toggleRestorePoint,
  };
}
