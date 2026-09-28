import { useCallback, useEffect, useMemo, useRef } from "react";
import { api } from "../lib/api";
import type { InstalledApp, ScanResult, CleanupReport, FullCleanupReport } from "../types";
import type { Strings } from "../i18n";
import { toast } from "../lib/toast";
import { formatError } from "../lib/format";
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
  // REV-FE-15: scan identity by content, not by name + item count — two scans
  // of the same app with the same count but different paths must re-trigger.
  const scanId = useMemo(() => {
    if (!scan) return null;
    let h = 5381;
    const feed = (s: string) => {
      for (let i = 0; i < s.length; i++) h = ((h << 5) + h + s.charCodeAt(i)) | 0;
    };
    feed(scan.app_name);
    for (const it of scan.items) feed(it.path);
    return `${scan.app_name}:${scan.items.length}:${h >>> 0}`;
  }, [scan]);
  const runAiExplain = useCallback(async (opts?: { force?: boolean }) => {
    if (!scan || !aiEnabled) return;
    // The auto effect runs right after force-clearing busy — the closure still
    // sees the pre-clear aiBusy, so the effect path passes force (seq already
    // supersedes any in-flight run; this effect owns the busy lifecycle).
    if (!opts?.force && aiBusy) return;
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
      // REV-FE-05: empty explain is not "AI disabled" — say so.
      if (!out.length) toast.info(L.aiEmptyResult);
    } catch (e) {
      // REV-SUP-05: surface the mapped backend cause, not one generic string.
      if (seq === aiExplainSeqRef.current) toast.error(formatError(e));
    } finally {
      if (seq === aiExplainSeqRef.current) setAiBusy(false);
    }
  }, [scan, selected, aiEnabled, aiBusy, L, setAiBusy, setAiNotes, setAiSummaryNote]);

  useEffect(() => {
    // Always invalidate in-flight explain when scan identity changes,
    // even if this run is skipped because aiBusy.
    aiExplainSeqRef.current += 1;
    // REV-FE-01: a superseded run's `finally` only clears busy when seq still matches.
    // Bumping seq above would otherwise leave aiBusy stuck true forever — force-clear here.
    setAiBusy(false);
    scanUi.clearAiSummary();
    scanUi.clearRiskFilter();
    aiActions.clearAiScanState();
    if (scan && scan.items.length > 0 && aiEnabled) {
      void runAiExplain({ force: true });
    }
    // scanId (REV-FE-15) fully identifies scan content; runAiExplain is stable
    // in the values it closes over.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scanId, aiEnabled]);

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
    // Same ownership rule as explain: invalidate then free busy so a stale finally cannot stick.
    aiReportSeqRef.current += 1;
    setAiReportBusy(false);
    if (report && aiEnabled) void runAiReport();
    // REV-FE-15: key on report identity fields, not object identity only.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [report, aiEnabled, report && "deleted" in report ? report.deleted : 0, report?.skipped]);

  return { runAiExplain, runAiReport };
}
