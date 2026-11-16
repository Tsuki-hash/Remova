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
import type { UninstallReview } from "../lib/uninstallReview";

export type AnalyzeOptions = {
  fromUninstall?: boolean;
  afterCleanup?: boolean;
  keepReport?: boolean;
  reviewSession?: ReviewSession;
};

/** Grouped setters for the analyze/uninstall flow (fewer flat parameters). */
export type AnalyzeFlowSetters = {
  setSelected: (a: InstalledApp | null) => void;
  setScanning: (v: boolean) => void;
  setScan: (r: ScanResult | null, target?: InstalledApp) => void;
  setReport: (r: CleanupReport | FullCleanupReport | null) => void;
  setAiNotes: (m: Record<string, string>) => void;
  setAiRisk: (v: string | null) => void;
  setIgnoreSuggestions: (s: IgnoreSuggestion[]) => void;
  setSelectedPaths: (s: Set<string>) => void;
  setError: (e: string | null) => void;
  setUninstallReview: (v: UninstallReview | null) => void;
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
    setUninstallingKey,
    setUninstallStage,
    setEvidence,
  } = flow;
  const analyzeSeqRef = useRef(0);
  /**
   * how many `analyze` calls currently hold `busyRef`. A plain
   * save/restore leaks when a newer analyze supersedes an older one (the older
   * one's `finally` is gated on its own `seq`, so it never runs), and a plain
   * clear steals the ref from `startUninstall`, which holds it across its own
   * nested `analyze` call. Depth plus "was it already held" covers both.
   */
  const analyzeHoldRef = useRef(0);
  const { run: runScan, cancel: cancelScan, progress: scanProgress } = useAssociationScan();
  const analyzingRef = useRef(false);
  /** Stage idle timers must be cleared on unmount / replacement. */
  const stageTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const clearStageTimer = useCallback(() => {
    if (stageTimerRef.current !== null) {
      clearTimeout(stageTimerRef.current);
      stageTimerRef.current = null;
    }
  }, []);
  useEffect(() => clearStageTimer, [clearStageTimer]);
  useEffect(() => () => { analyzeSeqRef.current++; }, []);
  const analyze = useCallback(
    async (
      app: InstalledApp,
      opts?: AnalyzeOptions,
    ) => {
 // single in-flight ref (was dual scanningRef/analyzingRef).
 // Newer analyze supersedes via seq — do not early-return here (race tests require it).
      // A cleanup/dry-run owns busyRef without analyzingRef. Starting an
      // unrelated scan then would let its report land on the wrong app; nested
      // uninstall/after-cleanup scans are explicitly allowed below.
      if (
        busyRef.current &&
        !analyzingRef.current &&
        !opts?.fromUninstall &&
        !opts?.afterCleanup
      ) {
        toast.info(t().taskBusy, { channel: "analyze-flow" });
        return false;
      }
      // Detect whether busyRef is held by a destructive action before this
      // analyze claims it. If another analyze already holds it, this call
      // inherits that claim and must release it when it becomes the survivor.
      const busyRefHeldExternally = busyRef.current && !analyzingRef.current;
      analyzingRef.current = true;
      const seq = ++analyzeSeqRef.current;
      // `forceClean` and `startUninstall` guard on `busyRef.current` only,
      // and it was never set here. A destructive action could therefore start
      // while a deep scan was in flight, and the scan then landed on top of the
      // other action's report (it never increments `analyzeSeqRef`). App.tsx's
      // "one guard" comment already claimed this ref is the single gate.
      analyzeHoldRef.current += 1;
      busyRef.current = true;
      goNav("software");
      const afterUninstall = Boolean(opts?.fromUninstall || opts?.afterCleanup);
      setUninstallReview(afterUninstall ? { app, rescan: { phase: "scanning" } } : null);
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
        const r = await runScan(app);
        if (seq !== analyzeSeqRef.current) return false;
        setScan(r, app);
        reviewRound = {
          items: r.items,
          complete: scanMatchesApp(r, app) && (r.degraded ?? []).length === 0,
        };
        if (afterUninstall) {
          setUninstallReview({ app, rescan: reviewRound.complete
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
        setSelectedPaths(keepReport ? new Set() : new Set(r.items.filter(defaultSelectable).map((it) => it.path)));
        setError(null);
        toast.success(
          t().toastAnalyzeDone(((performance.now() - t0) / 1000).toFixed(1), r.items.length),
          { channel: "analyze-flow", ttl: 3000 },
        );
        return true;
      } catch (e) {
        if (seq !== analyzeSeqRef.current) return false;
        const cancelled = scanCancelled(e);
        if (afterUninstall) setUninstallReview({ app, rescan: { phase: cancelled ? "cancelled" : "failed" } });
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
        analyzeHoldRef.current -= 1;
        // Release only when no analyze remains and this flow actually acquired
        // busyRef. A superseded scan must not clear a newer scan's claim, and a
        // scan nested inside startUninstall/cleanup must not steal that ref.
        if (analyzeHoldRef.current === 0 && !busyRefHeldExternally) busyRef.current = false;
        if (seq === analyzeSeqRef.current) {
          analyzingRef.current = false;
          setScanning(false);
        }
      }
    },
    [
      goNav,
      busyRef,
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
      setUninstallReview,
      runScan,
    ],
  );

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
        confirmLabel: strings.drawerDeepUninstall,
        checkbox: {
          label: strings.uninstallRescanAfter,
          defaultChecked: defaultRescan,
        },
      });
      if (!ok) return;
      if (busyRef.current) return;
 // kill the previous run's stage timer here — its 2.2s tail
 // would otherwise fire mid-run and flip the bar back to idle.
      clearStageTimer();
      saveRescanAfterUninstall(checked);
      const key = appKey(app);
      setUninstallingKey(key);
      setSelected(app);
      setUninstallReview(null);
      busyRef.current = true;
      try {
        setUninstallStage("identify");
        await new Promise((r) => setTimeout(r, 280));
        setUninstallStage("official");
        const r = await api.officialUninstall(app);
        const rebootPending = Boolean(r.had_command && r.ok && r.reboot_initiated);
        notifyUninstall(r, strings);
        if (r.had_command && r.ok) {
          setReport(null);
          setScan(null);
          setSelectedPaths(new Set());
          setUninstallReview({ app, rescan: { phase: rebootPending ? "reboot" : "not_started" } });
        }
        // MSI 1641 already started a restart: scanning is harmless, but offering
        // residual cleanup now would delete while the shutdown is pending.
        if (r.had_command && r.ok && checked && !rebootPending) {
          setUninstallStage("scan");
          toast.info(strings.stageScanLeftover, { channel: "analyze-flow", sticky: true });
          const analyzed = await analyze(app, { fromUninstall: true });
          if (analyzed) {
            setUninstallStage("analyze");
            await new Promise((resolve) => setTimeout(resolve, 320));
            setUninstallStage("report");
            clearStageTimer();
            stageTimerRef.current = setTimeout(() => setUninstallStage("idle"), 2200);
          } else {
            setUninstallStage("idle");
          }
        } else {
          setUninstallStage("idle");
        }
        await refreshApps();
      } catch (e) {
        clearStageTimer();
        setUninstallStage("idle");
        setError(formatError(e, "cleanup"));
        toast.error(formatError(e, "cleanup"), { channel: "analyze-flow" });
      } finally {
        busyRef.current = false;
        setUninstallingKey(null);
      }
    },
    [
      analyze,
      refreshApps,
      setSelected,
      setUninstallReview,
      setUninstallingKey,
      setUninstallStage,
      setReport,
      setScan,
      setSelectedPaths,
      setError,
      busyRef,
      clearStageTimer,
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
        confirmLabel: strings.drawerOfficial,
      });
      if (!ok) return;
      if (busyRef.current) return;
      // A previous stage timer must not fire during this run and flip the bar
      // back to idle before the official uninstaller finishes.
      clearStageTimer();
      const key = appKey(app);
      setUninstallingKey(key);
      setSelected(app);
      setUninstallReview(null);
      busyRef.current = true;
      try {
        const r = await api.officialUninstall(app);
        const rebootPending = Boolean(r.had_command && r.ok && r.reboot_initiated);
        notifyUninstall(r, strings);
        if (r.had_command && r.ok) {
          setReport(null);
          setScan(null);
          setSelectedPaths(new Set());
          setUninstallReview({ app, rescan: { phase: rebootPending ? "reboot" : "not_started" } });
        }
 // default-on rescan after successful official uninstall only.
        if (r.had_command && r.ok && !rebootPending && loadRescanAfterUninstall()) {
          setUninstallStage("scan");
          const analyzed = await analyze(app, { fromUninstall: true });
          if (analyzed) {
            setUninstallStage("analyze");
            await new Promise((resolve) => setTimeout(resolve, 280));
            setUninstallStage("report");
            clearStageTimer();
            stageTimerRef.current = setTimeout(() => setUninstallStage("idle"), 2000);
          } else {
            setUninstallStage("idle");
          }
        }
        await refreshApps();
      } catch (e) {
        setError(formatError(e, "cleanup"));
        toast.error(formatError(e, "cleanup"));
      } finally {
        busyRef.current = false;
        setUninstallingKey(null);
      }
    },
    [refreshApps, setSelected, setUninstallReview, setUninstallingKey, setError, busyRef, analyze, setUninstallStage, clearStageTimer, setReport, setScan, setSelectedPaths],
  );

  const openAnalyzeFromDrawer = useCallback(
    (app: InstalledApp) => {
      if (analyzingRef.current) return;
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
