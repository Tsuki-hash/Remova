import { useCallback, useState } from "react";
import type { InstalledApp } from "../types";
import type { VerifyRow } from "../lib/api";

/** AI / report / verify panel state + actions extracted from App. */
export function useAiPanelState() {
  const [aiEnabled, setAiEnabled] = useState(false);
  const [aiBusy, setAiBusy] = useState(false);
  const [aiNotes, setAiNotes] = useState<Record<string, string>>({});
  const [aiRisk, setAiRisk] = useState<string | null>(null);
  const [aiReportNote, setAiReportNote] = useState<string | null>(null);
  const [aiReportBusy, setAiReportBusy] = useState(false);
  const [verifyRows, setVerifyRows] = useState<VerifyRow[] | null>(null);
  const [copilotList, setCopilotList] = useState<InstalledApp[] | null>(null);

  const clearAiScanState = useCallback(() => {
    setAiNotes({});
    setAiRisk(null);
  }, []);

  const clearAiReport = useCallback(() => {
    setAiReportNote(null);
    setAiReportBusy(false);
  }, []);

  const applyCopilotFilter = useCallback((list: InstalledApp[]) => {
    setCopilotList(list);
  }, []);

  const clearCopilotFilter = useCallback(() => setCopilotList(null), []);

  return {
    aiEnabled,
    setAiEnabled,
    aiBusy,
    setAiBusy,
    aiNotes,
    setAiNotes,
    aiRisk,
    setAiRisk,
    aiReportNote,
    setAiReportNote,
    aiReportBusy,
    setAiReportBusy,
    verifyRows,
    setVerifyRows,
    copilotList,
    setCopilotList,
    actions: {
      clearAiScanState,
      clearAiReport,
      applyCopilotFilter,
      clearCopilotFilter,
    },
  };
}
