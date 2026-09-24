import { useCallback, useEffect, useRef } from "react";
import { api } from "../lib/api";
import type { InstalledApp, ScanResult, CleanupReport, FullCleanupReport } from "../types";
import type { Strings } from "../i18n";
import { toast } from "../lib/toast";
import { runAiReportSummary } from "../lib/aiNarrative";

type ScanChrome = {
  clearAiSummary: () => void;
  clearRiskFilter: () => void;
};

type AiScanActions = {
  clearAiScanState: () => void;
};

/**
 * AI leftover explanations + post-cleanup report narrative.
 * F-R7-03: extracted from App.tsx (race guards kept intact).
 */
export function useAiScanNarrative({
  scan,
  selected,
  report,
  aiEnabled,
  aiBusy,
  aiReportBusy,
  L,
  setAiBusy,
  setAiNotes,
  setAiSummaryNote,
  setAiReportBusy,
  setAiReportNote,
  scanUi,
  aiActions,
}: {
  scan: ScanResult | null;
  selected: InstalledApp | null;
  report: CleanupReport | FullCleanupReport | null;
  aiEnabled: boolean;
  aiBusy: boolean;
  aiReportBusy: boolean;
  L: Strings;
  setAiBusy: (v: boolean) => void;
  setAiNotes: (v: Record<string, string>) => void;
  setAiSummaryNote: (v: string | null) => void;
  setAiReportBusy: (v: boolean) => void;
  setAiReportNote: (v: string | null) => void;
  scanUi: ScanChrome;
  aiActions: AiScanActions;
}) {
  const aiExplainSeqRef = useRef(0);
  const runAiExplain = useCallback(async () => {
    if (!scan || !aiEnabled || aiBusy) return;
    const seq = ++aiExplainSeqRef.current;
    setAiBusy(true);
    try {
      const items = scan.items.slice(0, 12).map((it) => ({
        path: it.path,
        kind: String(it.kind),
        confidence: String(it.confidence),
        risk: String(it.risk),
        reason: it.reason,
        evidence_labels: (it.evidence || []).map((e) => e.label).filter(Boolean),
      }));
      const out = await api.aiExplain(scan.app_name, selected?.publisher || "", items);
      if (seq !== aiExplainSeqRef.current) return;
      const map: Record<string, string> = {};
      for (const o of out) {
        map[o.path] = o.summary;
      }
      setAiNotes(map);
      const brief = out
        .slice(0, 3)
        .map((o) => o.summary)
        .filter(Boolean)
        .join(" ");
      setAiSummaryNote(brief || null);
      if (!out.length) toast.info(L.aiDisabledHint);
    } catch {
      if (seq === aiExplainSeqRef.current) toast.error(L.aiFailed);
    } finally {
      if (seq === aiExplainSeqRef.current) setAiBusy(false);
    }
  }, [scan, selected, aiEnabled, aiBusy, L, setAiBusy, setAiNotes, setAiSummaryNote]);

  useEffect(() => {
    // Always invalidate in-flight explain when scan identity changes,
    // even if this run is skipped because aiBusy.
    aiExplainSeqRef.current += 1;
    scanUi.clearAiSummary();
    scanUi.clearRiskFilter();
    aiActions.clearAiScanState();
    if (scan && scan.items.length > 0 && aiEnabled) {
      void runAiExplain();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scan?.app_name, scan?.items.length, aiEnabled]);

  const aiReportSeqRef = useRef(0);
  const runAiReport = useCallback(async () => {
    if (!report || !("deleted" in report) || !aiEnabled || aiReportBusy) return;
    const seq = ++aiReportSeqRef.current;
    setAiReportBusy(true);
    try {
      const note = await runAiReportSummary(report);
      // F-R6-06: only the newest report summary may write aiReportNote.
      if (seq !== aiReportSeqRef.current) return;
      setAiReportNote(note);
    } catch {
      // rule narrative still shown
    } finally {
      if (seq === aiReportSeqRef.current) setAiReportBusy(false);
    }
  }, [report, aiEnabled, aiReportBusy, setAiReportBusy, setAiReportNote]);

  useEffect(() => {
    if (report && aiEnabled) void runAiReport();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [report, aiEnabled]);

  return { runAiExplain, runAiReport };
}
