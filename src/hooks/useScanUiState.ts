import { useCallback, useRef, useState } from "react";
import type { LinkedBucketId } from "../lib/linkedItems";

/**
 * Scan/conclusion UI chrome (PF-03): filters + AI nudge.
 * Kept out of App to shrink top-level state surface.
 */
export function useScanUiState() {
  const [kindFilter, setKindFilter] = useState<LinkedBucketId | null>(null);
  const pendingBucketFilter = useRef<LinkedBucketId | null>(null);
  const [riskFilter, setRiskFilter] = useState<"confirm" | "keep" | null>(null);
  const [aiSummaryNote, setAiSummaryNote] = useState<string | null>(null);
  const [aiNudgeDismissed, setAiNudgeDismissed] = useState(
    () => localStorage.getItem("remova_ai_nudge") === "1",
  );

  const clearKindFilter = useCallback(() => setKindFilter(null), []);
  const clearRiskFilter = useCallback(() => setRiskFilter(null), []);
  const clearAiSummary = useCallback(() => setAiSummaryNote(null), []);

  const setPendingBucket = useCallback((bucket: LinkedBucketId | null) => {
    pendingBucketFilter.current = bucket;
  }, []);

  const takePendingBucket = useCallback((): LinkedBucketId | null => {
    const v = pendingBucketFilter.current;
    pendingBucketFilter.current = null;
    return v;
  }, []);

  const toggleRiskFilter = useCallback((mode: "confirm" | "keep") => {
    setKindFilter(null);
    setRiskFilter((cur) => (cur === mode ? null : mode));
  }, []);

  const clearScanChrome = useCallback(() => {
    setKindFilter(null);
    setRiskFilter(null);
    setAiSummaryNote(null);
    pendingBucketFilter.current = null;
  }, []);

  const dismissAiNudge = useCallback(() => {
    setAiNudgeDismissed(true);
    localStorage.setItem("remova_ai_nudge", "1");
  }, []);

  return {
    kindFilter,
    setKindFilter,
    riskFilter,
    setRiskFilter,
    aiSummaryNote,
    setAiSummaryNote,
    aiNudgeDismissed,
    setAiNudgeDismissed,
    actions: {
      clearKindFilter,
      clearRiskFilter,
      clearAiSummary,
      setPendingBucket,
      takePendingBucket,
      toggleRiskFilter,
      clearScanChrome,
      dismissAiNudge,
    },
  };
}
