import { useCallback, useReducer } from "react";
import type { InstalledApp } from "../types";
import type { VerifyRow } from "../lib/api";
import { aiPanelReducer, initialAiPanelState } from "./reducers/aiPanel";

/** AI / report / verify panel state + actions (domain reducer). */
export function useAiPanelState() {
  const [state, dispatch] = useReducer(aiPanelReducer, undefined, initialAiPanelState);

  const setAiEnabled = useCallback((value: boolean) => {
    dispatch({ type: "enabled/set", value });
  }, []);
  const setAiBusy = useCallback((value: boolean) => {
    dispatch({ type: "busy/set", value });
  }, []);
  const setAiNotes = useCallback((value: Record<string, string>) => {
    dispatch({ type: "notes/set", value });
  }, []);
  const setAiRisk = useCallback((value: string | null) => {
    dispatch({ type: "risk/set", value });
  }, []);
  const setAiReportNote = useCallback((value: string | null) => {
    dispatch({ type: "reportNote/set", value });
  }, []);
  const setAiReportBusy = useCallback((value: boolean) => {
    dispatch({ type: "reportBusy/set", value });
  }, []);
  const setVerifyRows = useCallback((value: VerifyRow[] | null) => {
    dispatch({ type: "verifyRows/set", value });
  }, []);
  const setCopilotList = useCallback((value: InstalledApp[] | null) => {
    dispatch({ type: "copilot/set", value });
  }, []);
  const clearAiScanState = useCallback(() => dispatch({ type: "scanAi/clear" }), []);
  const clearAiReport = useCallback(() => dispatch({ type: "reportAi/clear" }), []);
  const applyCopilotFilter = useCallback((list: InstalledApp[]) => {
    dispatch({ type: "copilot/set", value: list });
  }, []);
  const clearCopilotFilter = useCallback(() => dispatch({ type: "copilot/set", value: null }), []);

  return {
    aiEnabled: state.aiEnabled,
    setAiEnabled,
    aiBusy: state.aiBusy,
    setAiBusy,
    aiNotes: state.aiNotes,
    setAiNotes,
    aiRisk: state.aiRisk,
    setAiRisk,
    aiReportNote: state.aiReportNote,
    setAiReportNote,
    aiReportBusy: state.aiReportBusy,
    setAiReportBusy,
    verifyRows: state.verifyRows,
    setVerifyRows,
    copilotList: state.copilotList,
    setCopilotList,
    actions: {
      clearAiScanState,
      clearAiReport,
      applyCopilotFilter,
      clearCopilotFilter,
      setAiEnabled,
      setAiBusy,
      setAiNotes,
      setAiRisk,
      setAiReportNote,
      setAiReportBusy,
      setVerifyRows,
    },
  };
}
