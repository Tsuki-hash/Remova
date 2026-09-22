import { useCallback, useMemo, useState } from "react";
import { api } from "../lib/api";
import type { InstalledApp } from "../types";
import type { Strings } from "../i18n";
import { formatError } from "../lib/format";
import { toast } from "../lib/toast";
import { isRecentInstall, summarizeLeftovers } from "../lib/decision";

/** Grouped chrome/tool setters (same shape as CleanupFlowSetters). */
export type AppChromeFlow = {
  setIgnorePub: (v: string[]) => void;
  setIgnoreName: (v: string[]) => void;
  setError: (e: string | null) => void;
  setScan: (r: { app_name: string; items: import("../types").CleanupItem[] } | null) => void;
  setSelected: (a: InstalledApp | null) => void;
};

export type AppChromeResidual = {
  clearSelection: () => void;
  setMonitoring: (v: boolean) => void;
  setMonitorDiff: (
    v: { added_files: string[]; added_reg_values: string[] } | null,
  ) => void;
  selectDefaultItems: (items: import("../types").CleanupItem[]) => void;
};

/** Checkup tile counts (large installs / recent installs). */
export function useCheckupStats(apps: InstalledApp[], sizeOf: (a: InstalledApp) => number) {
  return useMemo(() => {
    const large = apps.filter((a) => sizeOf(a) > 500 * 1024).length;
    const recent = apps.filter((a) => isRecentInstall(a.install_date, 30)).length;
    return { total: apps.length, large, recent };
  }, [apps, sizeOf]);
}

/**
 * Shell chrome / toolbox actions: ignore lists, orphan scan, install monitor,
 * checkup orphan count, elevate, open-path. F-R7-03: extracted from App.tsx.
 */
export function useAppChrome({
  L,
  selected,
  monitoring,
  goNav,
  flow,
  residual,
  setCheckupOrphanCount,
}: {
  L: Strings;
  selected: InstalledApp | null;
  monitoring: boolean;
  goNav: (n: "software") => void;
  flow: AppChromeFlow;
  residual: AppChromeResidual;
  setCheckupOrphanCount: (v: number | null) => void;
}) {
  /** F-R6-08: orphan checkup scan has its own spinner (never the analyze spinner). */
  const [checkupOrphanBusy, setCheckupOrphanBusy] = useState(false);

  const elevate = useCallback(async () => {
    try {
      await api.elevateRestart();
    } catch (e) {
      toast.error(formatError(e, "elevate"));
    }
  }, []);

  const openPathSafe = useCallback(async (path: string) => {
    try {
      await api.openPath(path);
    } catch (e) {
      toast.error(formatError(e));
    }
  }, []);

  const doIgnorePublisher = useCallback(
    async (appOverride?: InstalledApp) => {
      const pub = (appOverride ?? selected)?.publisher;
      if (!pub) return;
      try {
        const ig = await api.ignorePublisher(pub);
        flow.setIgnorePub(ig.publishers || []);
        toast.success(L.ignoreLoaded);
      } catch (e) {
        flow.setError(formatError(e));
      }
    },
    [selected, L, flow],
  );

  const doIgnoreApp = useCallback(
    async (appOverride?: InstalledApp) => {
      const name = (appOverride ?? selected)?.name;
      if (!name) return;
      try {
        const ig = await api.ignoreAppName(name);
        flow.setIgnoreName(ig.names || []);
        toast.success(L.ignoreLoaded);
      } catch (e) {
        flow.setError(formatError(e));
      }
    },
    [selected, L, flow],
  );

  const runOrphanScan = useCallback(async () => {
    toast.info(L.orphanScanning);
    try {
      const items = await api.orphanScan();
      goNav("software");
      flow.setScan({ app_name: L.orphanScan, items });
      residual.clearSelection();
      if (items.length === 0) toast.info(L.orphanScanEmpty);
      else {
        const s = summarizeLeftovers(items);
        toast.success(L.orphanScanDone(s.total, s.suggest, s.keep));
      }
    } catch (e) {
      flow.setError(formatError(e, "analyze"));
      toast.error(formatError(e, "analyze"));
    }
  }, [L, goNav, residual, flow]);

  const toggleMonitor = useCallback(async () => {
    try {
      if (!monitoring) {
        await api.beginInstallMonitor();
        residual.setMonitoring(true);
        residual.setMonitorDiff(null);
        toast.info(L.monitorRunning);
      } else {
        const d = await api.endInstallMonitor();
        residual.setMonitoring(false);
        residual.setMonitorDiff(d);
        toast.success(L.toastMonitorDiff(d.added_files.length, d.added_reg_values.length));
      }
    } catch (e) {
      flow.setError(formatError(e));
      toast.error(formatError(e));
      residual.setMonitoring(false);
    }
  }, [monitoring, L, residual, flow]);

  const monitorDiffToCleanup = useCallback(
    async (diff: { added_files: string[]; added_reg_values: string[] }) => {
      try {
        const items = await api.monitorDiffToItems(diff);
        if (!items.length) {
          toast.info(L.monitorNoSnap);
          return;
        }
        goNav("software");
        const monApp: InstalledApp = {
          name: L.monitorDiff,
          version: "",
          publisher: "",
          install_location: "",
          uninstall_string: "",
          quiet_uninstall_string: "",
          source: "Monitor",
          registry_key: "",
          estimated_size_kb: 0,
          install_date: "",
          display_icon: "",
        };
        flow.setSelected(monApp);
        flow.setScan({ app_name: L.monitorDiff, items });
        residual.selectDefaultItems(items);
        residual.setMonitorDiff(null);
        toast.success(`${L.monitorToCleanup}: ${items.length}`);
      } catch (e) {
        flow.setError(formatError(e));
      }
    },
    [L, goNav, residual, flow],
  );

  const checkupOrphanScan = useCallback(() => {
    // F-R6-08: independent spinner — never touch the analyze `scanning` flag.
    void (async () => {
      setCheckupOrphanBusy(true);
      try {
        const items = await api.orphanScan();
        setCheckupOrphanCount(items.length);
      } catch (e) {
        setCheckupOrphanCount(null);
        toast.error(formatError(e, "analyze"));
      } finally {
        setCheckupOrphanBusy(false);
      }
    })();
  }, [setCheckupOrphanCount]);

  return {
    checkupOrphanBusy,
    elevate,
    openPathSafe,
    doIgnorePublisher,
    doIgnoreApp,
    runOrphanScan,
    toggleMonitor,
    monitorDiffToCleanup,
    checkupOrphanScan,
  };
}
