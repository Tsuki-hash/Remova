import { cleanupItemKey } from "../lib/cleanupPlan";
import { useCallback, useEffect, useRef } from "react";
import { api } from "../lib/api";
import type { CleanupReport, FullCleanupReport, InstalledApp, OfficialUninstallResult, ScanResult } from "../types";
import { t } from "../i18n";
import { formatError, prettyAppName } from "../lib/format";
import { backendText, uninstallNeedsRestart } from "../lib/backendText";
import { requestConfirm, requestConfirmEx } from "../lib/confirm";
import { toast } from "../lib/toast";
import { defaultSelectable } from "../lib/decision";
import { appKey, scanMatchesApp } from "../lib/appKey";
import { loadRescanAfterUninstall, saveRescanAfterUninstall } from "../lib/rescanPref";
import { recordReviewRound, type ReviewSession } from "../lib/rescanRounds";
import type { ConvergenceRound } from "../lib/residueConvergence";
import type { IgnoreSuggestion } from "../types";
import type { UninstallStage } from "../components/UninstallStageBar";
import { useAssociationScan, scanCancelled } from "./useAssociationScan";
import { rescanFailure, uninstallReviewFromResult, type UninstallReview } from "../lib/uninstallReview";

export type AnalyzeOptions = {
  fromUninstall?: boolean;
  uninstallReview?: number;
  afterCleanup?: boolean;
  keepReport?: boolean;
  reviewSession?: ReviewSession;
};

/** Grouped setters for the analyze/uninstall flow (fewer flat parameters). */
export type AnalyzeFlowSetters = {
  setSelected: (a: InstalledApp | null) => void;
  setScanning: (v: boolean) => void;
  setScan: (r: ScanResult | null, target?: InstalledApp, reviewReceipt?: number) => void;
  setReport: (r: CleanupReport | FullCleanupReport | null) => void;
  setAiNotes: (m: Record<string, string>) => void;
  setAiRisk: (v: string | null) => void;
  setIgnoreSuggestions: (s: IgnoreSuggestion[]) => void;
  setSelectedPaths: (s: Set<string>) => void;
  setError: (e: string | null) => void;
  setUninstallReview: (v: UninstallReview) => void;
  updateUninstallRescan: (v: UninstallReview) => void;
  setUninstallingKey: (k: string | null) => void;
  setUninstallStage: (s: UninstallStage) => void;
  setEvidence: (v: string | null) => void;
};

function notifyUninstall(result: OfficialUninstallResult, L: ReturnType<typeof t>) {
  if (!result.had_command) {
    toast.info(L.uninstallNoCmd);
  } else if (result.reboot_initiated) {
    if (result.reboot_gate_persisted === false) toast.error(L.backendRebootGateNotPersisted);
    else toast.info(L.backendRebootInitiated);
  } else if (uninstallNeedsRestart(result.message)) {
    toast.info(backendText(result.message), { ttl: 7000 });
  } else if (result.ok) {
    toast.success(L.uninstallOk);
  } else if (result.message.startsWith("uninstall registration ")) {
    toast.info(backendText(result.message), { ttl: 7000 });
  } else if (result.message.includes("1602")) {
    toast.info(L.uninstallCancelled);
  } else {
    toast.error(L.uninstallFail, { detail: backendText(result.message) });
  }
}

/** Deep-analyze + official uninstall flow used by list rows and detail drawer. */
export function useAnalyzeFlow({
  goNav,
  flow,
  refreshApps,
  busyRef,
}: {
  goNav: (n: "software") => void;
  flow: AnalyzeFlowSetters;
  refreshApps: () => Promise<void>;
  busyRef: { current: boolean };
}) {
  const {
    setSelected,
    setScanning,
    setScan,
    setReport,
    setAiNotes,
    setAiRisk,
    setIgnoreSuggestions,
    setSelectedPaths,
    setError,
    setUninstallReview,
    updateUninstallRescan,
    setUninstallingKey,
    setUninstallStage,
    setEvidence,
  } = flow;
  const analyzeSeqRef = useRef(0);
  const { run: runScan, cancel: cancelScan, progress: scanProgress } = useAssociationScan();
  useEffect(() => () => { analyzeSeqRef.current++; }, []);
  // Only the holder of the foreground slot calls this. Official uninstall
  // keeps its slot through the nested scan and the installed-app refresh.
  const scanApp = useCallback(
    async (
      app: InstalledApp,
      opts?: AnalyzeOptions,
    ) => {
      const seq = ++analyzeSeqRef.current;
      goNav("software");
      const receipt = opts?.uninstallReview;
      const afterUninstall = receipt !== undefined;
      if (afterUninstall) updateUninstallRescan({ app, receipt, rescan: { phase: "scanning" } });
      const keepReport = Boolean(opts?.afterCleanup || opts?.keepReport);
      setSelected(app);
      setScanning(true);
      setScan(null);
      setSelectedPaths(new Set());
      setError(null);
      if (!keepReport) setReport(null);
      setAiNotes({});
      setAiRisk(null);
      setIgnoreSuggestions([]);
 // A new app's scan must not inherit the previous app's evidence bar.
      setEvidence(null);
      const t0 = performance.now();
      let reviewRound: ConvergenceRound = { items: [], complete: false };
      try {
        const r = await runScan(app, receipt);
        if (seq !== analyzeSeqRef.current) return false;
        setScan(r, app, receipt);
        reviewRound = {
          items: r.items,
          complete: scanMatchesApp(r, app) && (r.degraded ?? []).length === 0,
        };
        if (afterUninstall) {
          updateUninstallRescan({ app, receipt, rescan: reviewRound.complete
            ? { phase: "complete", count: r.items.length } : { phase: "incomplete" } });
        }
        const sharedPaths = r.items.filter((it) => it.shared).map((it) => it.path);
        if (sharedPaths.length > 0 && app.publisher) {
          void api
            .suggestIgnoreRules(app.publisher, sharedPaths)
            .then((sugs) => {
              if (seq === analyzeSeqRef.current) setIgnoreSuggestions(sugs || []);
            })
            .catch(() => {});
        }
        setSelectedPaths(keepReport ? new Set() : new Set(r.items.filter(defaultSelectable).map(cleanupItemKey)));
        setError(null);
        toast.success(
          t().toastAnalyzeDone(((performance.now() - t0) / 1000).toFixed(1), r.items.length),
          { channel: "analyze-flow", ttl: 3000 },
        );
        return true;
      } catch (e) {
        if (seq !== analyzeSeqRef.current) return false;
        const cancelled = scanCancelled(e);
        if (afterUninstall) updateUninstallRescan({ app, receipt, rescan: rescanFailure(e) });
        if (cancelled) {
          setSelectedPaths(new Set());
          setError(null);
          toast.info(t().scanCancelledIncomplete, { channel: "analyze-flow" });
          return false;
        }
        setError(formatError(e, "analyze"));
        toast.error(formatError(e, "analyze"), { channel: "analyze-flow" });
        return false;
      } finally {
        if (opts?.reviewSession) recordReviewRound(opts.reviewSession, reviewRound);
        if (seq === analyzeSeqRef.current) setScanning(false);
      }
    },
    [
      goNav,
      setSelected,
      setScanning,
      setScan,
      setReport,
      setAiNotes,
      setAiRisk,
      setIgnoreSuggestions,
      setSelectedPaths,
      setEvidence,
      setError,
      updateUninstallRescan,
      runScan,
    ],
  );

  const analyze = useCallback(async (app: InstalledApp, opts?: AnalyzeOptions) => {
    if (busyRef.current) {
      toast.info(t().taskBusy, { channel: "analyze-flow" });
      return false;
    }
    busyRef.current = true;
    try {
      return await scanApp(app, opts);
    } finally {
      busyRef.current = false;
    }
  }, [busyRef, scanApp]);

  const rememberUninstall = useCallback((app: InstalledApp, result: OfficialUninstallResult) => {
    if (!result.had_command) return;
    const review = uninstallReviewFromResult(app, {
      ok: result.ok, message: result.message, reboot: result.reboot_initiated, receipt: result.uninstall_review,
    });
    if (!review) return;
    setReport(null);
    setScan(null);
    setSelectedPaths(new Set());
    setUninstallReview(review);
  }, [setReport, setScan, setSelectedPaths, setUninstallReview]);

  const startUninstall = useCallback(
    async (app: InstalledApp) => {
      if (busyRef.current) return;
      const strings = t();
      const label = prettyAppName(app.name, app.source);
      const steps = [
        strings.uninstallFlowStepOfficial,
        strings.uninstallFlowStepScan,
        strings.uninstallFlowStepAnalyze,
        strings.uninstallFlowStepConfirm,
        strings.uninstallFlowStepBackup,
      ];
      const defaultRescan = loadRescanAfterUninstall();
      const command = (app.uninstall_string || app.quiet_uninstall_string || "").trim();
      const commandLine = command
        ? `\n\n${strings.uninstallCommand}: ${command.length > 180 ? `${command.slice(0, 180)}…` : command}`
        : "";
      const { ok, checked } = await requestConfirmEx({
        title: strings.uninstallConfirmDeepTitle || strings.uninstallFlowDeep,
        message: `${strings.uninstallConfirmDeepBody(label)}\n\n${steps.map((s, i) => `${i + 1}. ${s}`).join("\n")}${commandLine}`,
        confirmLabel: strings.uninstallFlowStepOfficial,
        checkbox: {
          label: strings.uninstallRescanAfter,
          defaultChecked: defaultRescan,
        },
      });
      if (!ok) return;
      if (busyRef.current) return;
      saveRescanAfterUninstall(checked);
      const key = appKey(app);
      setUninstallingKey(key);
      setSelected(app);
      busyRef.current = true;
      try {
        setUninstallStage("official");
        const r = await api.officialUninstall(app);
        setUninstallStage("idle");
        const rebootPending = Boolean(r.had_command && r.ok && r.reboot_initiated);
        notifyUninstall(r, strings);
        rememberUninstall(app, r);
        // MSI 1641 already started a restart: scanning is harmless, but offering
        // residual cleanup now would delete while the shutdown is pending.
        if (r.had_command && r.ok && checked && !rebootPending && r.uninstall_review !== undefined) {
          await scanApp(app, { uninstallReview: r.uninstall_review });
        }
        await refreshApps();
      } catch (e) {
        setError(formatError(e, "cleanup"));
        toast.error(formatError(e, "cleanup"), { channel: "analyze-flow" });
      } finally {
        setUninstallStage("idle");
        busyRef.current = false;
        setUninstallingKey(null);
      }
    },
    [
      scanApp,
      refreshApps,
      setSelected,
      rememberUninstall,
      setUninstallingKey,
      setUninstallStage,
      setError,
      busyRef,
    ],
  );

  const openDeepFromDrawer = useCallback(
    (app: InstalledApp) => {
      void startUninstall(app);
    },
    [startUninstall],
  );

const openOfficialOnly = useCallback(
    async (app: InstalledApp) => {
      if (busyRef.current) return;
      const strings = t();
      const label = prettyAppName(app.name, app.source);
      const ok = await requestConfirm({
        title: strings.drawerOfficial,
        message: strings.uninstallConfirm(label),
        confirmLabel: strings.uninstallFlowStepOfficial,
      });
      if (!ok) return;
      if (busyRef.current) return;
      const key = appKey(app);
      setUninstallingKey(key);
      setSelected(app);
      busyRef.current = true;
      try {
        setUninstallStage("official");
        const r = await api.officialUninstall(app);
        setUninstallStage("idle");
        const rebootPending = Boolean(r.had_command && r.ok && r.reboot_initiated);
        notifyUninstall(r, strings);
        rememberUninstall(app, r);
 // default-on rescan after successful official uninstall only.
        if (r.had_command && r.ok && !rebootPending && r.uninstall_review !== undefined && loadRescanAfterUninstall()) {
          await scanApp(app, { uninstallReview: r.uninstall_review });
        }
        await refreshApps();
      } catch (e) {
        setError(formatError(e, "cleanup"));
        toast.error(formatError(e, "cleanup"));
      } finally {
        setUninstallStage("idle");
        busyRef.current = false;
        setUninstallingKey(null);
      }
    },
    [refreshApps, setSelected, rememberUninstall, setUninstallingKey, setError, busyRef, scanApp, setUninstallStage],
  );

  const openAnalyzeFromDrawer = useCallback(
    (app: InstalledApp) => {
      void analyze(app);
    },
    [analyze],
  );

  const selectApp = useCallback(
    (app: InstalledApp) => {
      if (busyRef.current) {
        toast.info(t().taskBusy, { channel: "analyze-flow" });
        return;
      }
      setSelected(app);
      // A previous app's scan (and its selected paths) must not stay on screen
      // or be submitted for the newly selected app.
      setScan(null);
      setSelectedPaths(new Set());
    },
    [busyRef, setSelected, setScan, setSelectedPaths],
  );

  return {
    analyze,
    cancelScan,
    scanProgress,
    startUninstall,
    openDeepFromDrawer,
    openOfficialOnly,
    openAnalyzeFromDrawer,
    selectApp,
  };
}
