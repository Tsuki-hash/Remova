import { useCallback, useMemo, useRef, useState } from "react";
import { api } from "../lib/api";
import type { InstalledApp } from "../types";
import type { Strings } from "../i18n";
import { t } from "../i18n";
import { useAssociationScan, scanCancelled } from "./useAssociationScan";
import { formatError } from "../lib/format";
import { requestConfirm } from "../lib/confirm";
import { toast } from "../lib/toast";
import { isRecentInstall } from "../lib/decision";

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
    v: {
      added_files: string[];
      added_reg_values: string[];
      items?: import("../types").CleanupItem[];
    } | null,
  ) => void;
  monitorDiff: {
    added_files: string[];
    added_reg_values: string[];
    items?: import("../types").CleanupItem[];
  } | null;
  selectDefaultItems: (items: import("../types").CleanupItem[]) => void;
};

import { LARGE_APP_KB } from "../lib/decision";

/** Checkup tile counts (large installs / recent installs). */
export function useCheckupStats(apps: InstalledApp[], sizeOf: (a: InstalledApp) => number) {
  return useMemo(() => {
    const large = apps.filter((a) => sizeOf(a) > LARGE_APP_KB).length;
    const recent = apps.filter((a) => isRecentInstall(a.install_date, 30)).length;
    return { total: apps.length, large, recent };
  }, [apps, sizeOf]);
}

/**
 * Shell chrome / toolbox actions: ignore lists, orphan scan, install monitor,
 * checkup orphan count, elevate, open-path. -03: extracted from App.tsx.
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
  /** -08: orphan checkup scan has its own spinner (never the analyze spinner). */
  const [checkupOrphanBusy, setCheckupOrphanBusy] = useState(false);
  const checkupScan = useAssociationScan();
  const monitorScan = useAssociationScan();
  const checkupBusyRef = useRef(false);
  const monitorBusyRef = useRef(false);

  const elevate = useCallback(async () => {
    // The badge fires UAC straight from a header chip — require the same
    // explicit consent the manage pages ask for before relaunching.
    const L = t();
    const ok = await requestConfirm({
      title: L.elevateAskTitle,
      message: L.elevateAskBody,
      confirmLabel: L.elevateAskOk,
    });
    if (!ok) return;
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
        return ig;
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

  const toggleMonitor = useCallback(async () => {
 // Repeated clicks during the filesystem snapshot used to queue work and
 // then dump a pile of toasts — one run at a time, one toast channel.
    if (monitorBusyRef.current) return;
    monitorBusyRef.current = true;
    residual.setMonitorDiff(null);
    if (selected?.source === "Monitor") { flow.setScan(null); residual.clearSelection(); }
    const MON_CH = "install-monitor";
    try {
      if (!monitoring) {
        toast.info(L.monitorStarting, { channel: MON_CH });
        await monitorScan.runReadOnly(api.beginInstallMonitor, () => 0);
        residual.setMonitoring(true);
        residual.setMonitorDiff(null);
        toast.success(L.monitorRunning, { channel: MON_CH, ttl: 3000 });
      } else {
        toast.info(L.monitorFinishing, { channel: MON_CH });
        const result = await monitorScan.runReadOnly(api.endInstallMonitor, result => result.diff.added_files.length + result.diff.added_reg_values.length);
        residual.setMonitoring(false);
        residual.setMonitorDiff({ ...result.diff, items: result.items });
        toast.success(
          L.toastMonitorDiff(result.diff.added_files.length, result.diff.added_reg_values.length),
          {
            channel: MON_CH,
            ttl: 3000,
          },
        );
      }
    } catch (e) {
      if (scanCancelled(e)) {
        residual.setMonitoring(monitoring);
        toast.info(L.monitorScanCancelled(monitoring), { channel: MON_CH });
        return;
      }
      flow.setError(formatError(e));
      toast.error(formatError(e), { channel: MON_CH });
      const raw = typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
      residual.setMonitoring(monitoring && raw !== "no monitor snapshot; start first");
    } finally {
      monitorBusyRef.current = false;
    }
  }, [monitoring, L, residual, flow, selected, monitorScan.runReadOnly]);

  const monitorDiffToCleanup = useCallback(
    (_diff: { added_files: string[]; added_reg_values: string[] }) => {
      try {
 // Trusted items only: captured when the server finished the monitor snapshot.
        const items = residual.monitorDiff?.items ?? [];
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
    if (checkupBusyRef.current) return;
    checkupBusyRef.current = true;
    setCheckupOrphanCount(null);
 // -08: independent spinner — never touch the analyze `scanning` flag.
    void (async () => {
      setCheckupOrphanBusy(true);
      try {
        const items = await checkupScan.runItems(api.orphanScan);
        setCheckupOrphanCount(items.length);
      } catch (e) {
        setCheckupOrphanCount(null);
        if (!scanCancelled(e)) toast.error(formatError(e, "analyze"));
      } finally {
        setCheckupOrphanBusy(false);
        checkupBusyRef.current = false;
      }
    })();
  }, [setCheckupOrphanCount, checkupScan.runItems]);

  return {
    checkupOrphanBusy,
    checkupScanProgress: checkupScan.progress,
    cancelCheckupScan: checkupScan.cancel,
    monitorScanProgress: monitorScan.progress,
    cancelMonitorScan: monitorScan.cancel,
    elevate,
    openPathSafe,
    doIgnorePublisher,
    doIgnoreApp,
    toggleMonitor,
    monitorDiffToCleanup,
    checkupOrphanScan,
  };
}
