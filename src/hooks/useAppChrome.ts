import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type InstallMonitorStatus, type MonitorDiff } from "../lib/api";
import type { InstalledApp } from "../types";
import type { Strings } from "../i18n";
import { t } from "../i18n";
import { useAssociationScan, scanCancelled } from "./useAssociationScan";
import { formatError } from "../lib/format";
import { requestConfirm } from "../lib/confirm";
import { toast } from "../lib/toast";
import { isRecentInstall } from "../lib/decision";
import { normalizeOrphanScan } from "../lib/scopedScan";

/** Grouped chrome/tool setters (same shape as CleanupFlowSetters). */
export type AppChromeFlow = {
  setIgnorePub: (v: string[]) => void;
  setIgnoreName: (v: string[]) => void;
  setError: (e: string | null) => void;
  setScan: (r: import("../types").ScanResult | null) => void;
  setSelected: (a: InstalledApp | null) => void;
};

export type AppChromeResidual = {
  clearSelection: () => void;
  setMonitoring: (v: boolean) => void;
  setMonitorDiff: (
    v: (MonitorDiff & {
      items?: import("../types").CleanupItem[];
      scope_revision?: number;
    }) | null,
  ) => void;
  monitorDiff: (MonitorDiff & {
    items?: import("../types").CleanupItem[];
    scope_revision?: number;
  }) | null;
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
 * checkup orphan count, elevate, open-path. Extracted from App.tsx.
 */
export function useAppChrome({
  L,
  selected,
  monitoring,
  busyRef,
  goNav,
  flow,
  residual,
  setCheckupOrphanCount,
  setCheckupOrphanCompleteness,
}: {
  L: Strings;
  selected: InstalledApp | null;
  monitoring: boolean;
  busyRef: { current: boolean };
  goNav: (n: "software") => void;
  flow: AppChromeFlow;
  residual: AppChromeResidual;
  setCheckupOrphanCount: (v: number | null) => void;
  setCheckupOrphanCompleteness: (v: import("../types").ScanCompleteness | null) => void;
}) {
  /** Orphan checkup scan has its own spinner (never the analyze spinner). */
  const [checkupOrphanBusy, setCheckupOrphanBusy] = useState(false);
  const checkupScan = useAssociationScan();
  const monitorScan = useAssociationScan();
  // bind the stable callbacks, so the dependency arrays can name them
  // without depending on the hook's re-created container object.
  const checkupRunReadOnly = checkupScan.runReadOnly;
  const monitorRunReadOnly = monitorScan.runReadOnly;
  const checkupBusyRef = useRef(false);
  const monitorBusyRef = useRef(false);
  const [monitorRecovery, setMonitorRecovery] = useState<"recovered" | "unavailable" | null>(null);
  const setMonitoring = residual.setMonitoring;
  const applyMonitorStatus = useCallback((status: InstallMonitorStatus) => {
    setMonitoring(status === "active");
    setMonitorRecovery(status === "recovered" || status === "unavailable" ? status : null);
  }, [setMonitoring]);
  useEffect(() => {
    let mounted = true;
    void api.installMonitorStatus().then(status => {
      if (mounted && !monitorBusyRef.current) applyMonitorStatus(status);
    }).catch(() => {
      if (mounted && !monitorBusyRef.current) setMonitorRecovery("unavailable");
    });
    return () => { mounted = false; };
  }, [applyMonitorStatus]);

  const discardMonitorRecovery = useCallback(async () => {
    if (monitorBusyRef.current || !monitorRecovery) return;
    monitorBusyRef.current = true;
    try {
      if (!await requestConfirm({ title: L.monitorDiscardRecovery, message: L.monitorDiscardPrompt, confirmLabel: L.monitorDiscardRecovery })) return;
      await api.discardInstallMonitorRecovery();
      applyMonitorStatus("idle");
    } catch (error) {
      toast.error(formatError(error));
    } finally {
      monitorBusyRef.current = false;
    }
  }, [L, applyMonitorStatus, monitorRecovery]);

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
      const app = appOverride ?? selected;
      // without an app this used to return undefined into a `pending`
      // mutation — a spinner with no action and no feedback.
      if (!app) {
        toast.info(L.selectRowHint);
        return;
      }
      const pub = app.publisher;
      if (!pub) {
        toast.info(L.ignorePublisherMissing);
        return;
      }
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

  /**
   * `toggleMonitor` and `checkupOrphanScan` are the two flows here that
   * issue cancellable async work, and neither carries a local request counter.
   * That is deliberate — both run through `useAssociationScan`, which already
   * owns a job token per run and refuses to let a superseded run publish
   * (`active.current !== job` → `scan:cancelled`). Adding a second counter here
   * would be the duplication this hook exists to remove, not a fix for it.
   */
  const toggleMonitor = useCallback(async (trialName?: string) => {
 // Repeated clicks during the filesystem snapshot used to queue work and
 // then dump a pile of toasts — one run at a time, one toast channel.
    if (monitorBusyRef.current || monitorRecovery === "unavailable") return;
    monitorBusyRef.current = true;
    residual.setMonitorDiff(null);
    if (selected?.source === "Monitor") { flow.setScan(null); residual.clearSelection(); }
    const MON_CH = "install-monitor";
    try {
      if (!monitoring && monitorRecovery !== "recovered") {
        toast.info(L.monitorStarting, { channel: MON_CH });
        await monitorRunReadOnly(id => api.beginInstallMonitor(id, trialName), () => 0);
        residual.setMonitoring(true);
        residual.setMonitorDiff(null);
        toast.success(L.monitorRunning, { channel: MON_CH, ttl: 3000 });
      } else {
        toast.info(L.monitorFinishing, { channel: MON_CH });
        const result = await monitorRunReadOnly(api.endInstallMonitor, result => result.diff.added_files.length + result.diff.added_reg_values.length);
        residual.setMonitoring(false);
        setMonitorRecovery(null);
        residual.setMonitorDiff({ ...result.diff, items: result.items, scope_revision: result.scope_revision });
        if (result.diff.trial_history_save_failed) toast.info(L.trialHistoryNotSaved, { channel: MON_CH });
        else toast.success(
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
      const raw = typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
      const monitorErrors: Partial<Record<string, string>> = {
        "monitor:checkpoint_pending": L.monitorRecoveryUnavailable,
        "monitor:checkpoint_unavailable": L.monitorRecoveryUnavailable,
        "monitor:checkpoint_write_failed": L.monitorRecoverySaveFailed,
        "trial:invalid_name": L.trialInvalidName,
        "trial:unavailable": L.trialSaveFailed,
      };
      const message = monitorErrors[raw] ?? formatError(e);
      flow.setError(message);
      toast.error(message, { channel: MON_CH });
      if (raw === "no monitor snapshot; start first" || raw.startsWith("monitor:checkpoint_")) {
        await api.installMonitorStatus().then(applyMonitorStatus).catch(() => setMonitorRecovery("unavailable"));
      }
    } finally {
      monitorBusyRef.current = false;
    }
  }, [monitoring, monitorRecovery, L, residual, flow, selected, monitorRunReadOnly, applyMonitorStatus]);

  const monitorDiffToCleanup = useCallback(
    (_diff: { added_files: string[]; added_reg_values: string[] }) => {
      if (busyRef.current) {
        toast.info(L.taskBusy);
        return;
      }
      busyRef.current = true;
      try {
 // Trusted items only: captured when the server finished the monitor snapshot.
        const result = residual.monitorDiff;
        const items = result?.trial_read_only || result?.recovered_read_only ? [] : result?.items ?? [];
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
        flow.setScan({ app_name: L.monitorDiff, items, scope_revision: residual.monitorDiff?.scope_revision });
        residual.selectDefaultItems(items);
        residual.setMonitorDiff(null);
        toast.success(`${L.monitorToCleanup}: ${items.length}`);
      } catch (e) {
        flow.setError(formatError(e));
      } finally {
        busyRef.current = false;
      }
    },
    [L, goNav, residual, flow, busyRef],
  );

  // Second of the two cases: guarded by `useAssociationScan`'s job token
  // (see the note on `toggleMonitor`), so no local request counter here either.
  const checkupOrphanScan = useCallback(() => {
    if (checkupBusyRef.current) return;
    checkupBusyRef.current = true;
    setCheckupOrphanCount(null);
    setCheckupOrphanCompleteness(null);
 // Independent spinner — never touch the analyze `scanning` flag.
    void (async () => {
      setCheckupOrphanBusy(true);
      try {
        const raw = await checkupRunReadOnly(
          (id: number) => api.orphanScan(id),
          (r) => normalizeOrphanScan(r).items.length,
        );
        const normalized = normalizeOrphanScan(raw);
        setCheckupOrphanCount(normalized.items.length);
        // A partial scan found N so far, not N total: the panel must not show
        // an incomplete count as a finished one.
        setCheckupOrphanCompleteness(normalized.completeness);
      } catch (e) {
        setCheckupOrphanCount(null);
        if (!scanCancelled(e)) toast.error(formatError(e, "analyze"));
      } finally {
        setCheckupOrphanBusy(false);
        checkupBusyRef.current = false;
      }
    })();
  }, [setCheckupOrphanCount, setCheckupOrphanCompleteness, checkupRunReadOnly]);

  return {
    checkupOrphanBusy,
    checkupScanProgress: checkupScan.progress,
    cancelCheckupScan: checkupScan.cancel,
    monitorScanProgress: monitorScan.progress,
    cancelMonitorScan: monitorScan.cancel,
    monitorRecovery,
    discardMonitorRecovery,
    elevate,
    openPathSafe,
    doIgnorePublisher,
    doIgnoreApp,
    toggleMonitor,
    monitorDiffToCleanup,
    checkupOrphanScan,
  };
}
