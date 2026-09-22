import { useCallback, useMemo, useReducer } from "react";
import type { CleanupItem, IgnoreSuggestion } from "../types";
import type { VerifyRow } from "../lib/api";
import { initialResidualState, residualReducer } from "./reducers/residual";

export type { VerifyRow };

/** Residual / ignore / monitor state + actions (domain reducer). */
export function useResidualState() {
  const [state, dispatch] = useReducer(residualReducer, undefined, initialResidualState);

  const togglePath = useCallback((path: string) => {
    dispatch({ type: "selection/toggle", path });
  }, []);

  /** True React functional updates (FE-P0a). */
  const setSelectedPaths = useCallback(
    (updater: Set<string> | ((s: Set<string>) => Set<string>)) => {
      if (typeof updater === "function") {
        dispatch({ type: "selection/update", updater });
      } else {
        dispatch({ type: "selection/set", paths: updater });
      }
    },
    [],
  );
  const selectDefaultItems = useCallback((items: CleanupItem[]) => {
    dispatch({ type: "selection/defaultItems", items });
  }, []);
  const clearSelection = useCallback(() => dispatch({ type: "selection/clear" }), []);
  const clearIgnoreSuggestions = useCallback(() => dispatch({ type: "ignore/clear" }), []);
  const clearResidualScan = useCallback(() => dispatch({ type: "scanChrome/clear" }), []);
  const setEvidence = useCallback((value: string | null) => {
    dispatch({ type: "evidence/set", value });
  }, []);
  const setIgnoreSuggestions = useCallback((value: IgnoreSuggestion[]) => {
    dispatch({ type: "ignore/set", value });
  }, []);
  const setMonitoring = useCallback((value: boolean) => {
    dispatch({ type: "monitoring/set", value });
  }, []);
  const setMonitorDiff = useCallback(
    (value: { added_files: string[]; added_reg_values: string[] } | null) => {
      dispatch({ type: "monitorDiff/set", value });
    },
    [],
  );
  const setResidualFromUninstall = useCallback((value: boolean) => {
    dispatch({ type: "residualFromUninstall/set", value });
  }, []);

  const actions = useMemo(
    () => ({
      togglePath,
      selectDefaultItems,
      clearSelection,
      clearIgnoreSuggestions,
      clearResidualScan,
      setEvidence,
      setIgnoreSuggestions,
      setMonitoring,
      setMonitorDiff,
      setResidualFromUninstall,
    }),
    [
      togglePath,
      selectDefaultItems,
      clearSelection,
      clearIgnoreSuggestions,
      clearResidualScan,
      setEvidence,
      setIgnoreSuggestions,
      setMonitoring,
      setMonitorDiff,
      setResidualFromUninstall,
    ],
  );

  return useMemo(
    () => ({
      selectedPaths: state.selectedPaths,
      setSelectedPaths,
      evidence: state.evidence,
      setEvidence,
      ignoreSuggestions: state.ignoreSuggestions,
      setIgnoreSuggestions,
      monitoring: state.monitoring,
      setMonitoring,
      monitorDiff: state.monitorDiff,
      setMonitorDiff,
      residualFromUninstall: state.residualFromUninstall,
      setResidualFromUninstall,
      actions,
    }),
    [
      state,
      setSelectedPaths,
      setEvidence,
      setIgnoreSuggestions,
      setMonitoring,
      setMonitorDiff,
      setResidualFromUninstall,
      actions,
    ],
  );
}
