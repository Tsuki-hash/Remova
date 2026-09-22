import { useCallback, useMemo, useReducer } from "react";
import type { LinkedBucketId } from "../lib/linkedItems";
import { initialScanUiState, scanUiReducer } from "./reducers/scanUi";

/** Scan/conclusion UI chrome (PF-03) — domain reducer. */
export function useScanUiState() {
  const [state, dispatch] = useReducer(scanUiReducer, undefined, initialScanUiState);

  const setKindFilter = useCallback((value: LinkedBucketId | null) => {
    dispatch({ type: "kindFilter/set", value });
  }, []);
  const setRiskFilter = useCallback((value: "confirm" | "keep" | null) => {
    dispatch({ type: "riskFilter/set", value });
  }, []);
  const setAiSummaryNote = useCallback((value: string | null) => {
    dispatch({ type: "aiSummary/set", value });
  }, []);
  const clearKindFilter = useCallback(() => dispatch({ type: "kindFilter/set", value: null }), []);
  const clearRiskFilter = useCallback(() => dispatch({ type: "riskFilter/set", value: null }), []);
  const clearAiSummary = useCallback(() => dispatch({ type: "aiSummary/set", value: null }), []);
  const setPendingBucket = useCallback((bucket: LinkedBucketId | null) => {
    dispatch({ type: "pendingBucket/set", value: bucket });
  }, []);
  const takePendingBucket = useCallback((): LinkedBucketId | null => {
    const v = state.pendingBucket;
    dispatch({ type: "pendingBucket/take" });
    return v;
  }, [state.pendingBucket]);
  const toggleRiskFilter = useCallback((mode: "confirm" | "keep") => {
    dispatch({ type: "riskFilter/toggle", mode });
  }, []);
  const clearScanChrome = useCallback(() => dispatch({ type: "chrome/clear" }), []);
  const dismissAiNudge = useCallback(() => {
    localStorage.setItem("remova_ai_nudge", "1");
    dispatch({ type: "aiNudge/dismiss" });
  }, []);

  // stable object identity — SoftwarePage memo depends on this bag.
  return useMemo(
    () => ({
      kindFilter: state.kindFilter,
      setKindFilter,
      riskFilter: state.riskFilter,
      setRiskFilter,
      aiSummaryNote: state.aiSummaryNote,
      setAiSummaryNote,
      aiNudgeDismissed: state.aiNudgeDismissed,
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
    }),
    [
      state.kindFilter,
      state.riskFilter,
      state.aiSummaryNote,
      state.aiNudgeDismissed,
      setKindFilter,
      setRiskFilter,
      setAiSummaryNote,
      clearKindFilter,
      clearRiskFilter,
      clearAiSummary,
      setPendingBucket,
      takePendingBucket,
      toggleRiskFilter,
      clearScanChrome,
      dismissAiNudge,
    ],
  );
}
