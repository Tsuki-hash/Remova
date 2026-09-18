import type { InstalledApp } from "../../types";
import type { VerifyRow } from "../../lib/api";

export type AiPanelState = {
  aiEnabled: boolean;
  aiBusy: boolean;
  aiNotes: Record<string, string>;
  aiRisk: string | null;
  aiReportNote: string | null;
  aiReportBusy: boolean;
  verifyRows: VerifyRow[] | null;
  copilotList: InstalledApp[] | null;
};

export type AiPanelAction =
  | { type: "enabled/set"; value: boolean }
  | { type: "busy/set"; value: boolean }
  | { type: "notes/set"; value: Record<string, string> }
  | { type: "risk/set"; value: string | null }
  | { type: "reportNote/set"; value: string | null }
  | { type: "reportBusy/set"; value: boolean }
  | { type: "verifyRows/set"; value: VerifyRow[] | null }
  | { type: "copilot/set"; value: InstalledApp[] | null }
  | { type: "scanAi/clear" }
  | { type: "reportAi/clear" };

export function initialAiPanelState(): AiPanelState {
  return {
    aiEnabled: false,
    aiBusy: false,
    aiNotes: {},
    aiRisk: null,
    aiReportNote: null,
    aiReportBusy: false,
    verifyRows: null,
    copilotList: null,
  };
}

export function aiPanelReducer(state: AiPanelState, action: AiPanelAction): AiPanelState {
  switch (action.type) {
    case "enabled/set":
      return { ...state, aiEnabled: action.value };
    case "busy/set":
      return { ...state, aiBusy: action.value };
    case "notes/set":
      return { ...state, aiNotes: action.value };
    case "risk/set":
      return { ...state, aiRisk: action.value };
    case "reportNote/set":
      return { ...state, aiReportNote: action.value };
    case "reportBusy/set":
      return { ...state, aiReportBusy: action.value };
    case "verifyRows/set":
      return { ...state, verifyRows: action.value };
    case "copilot/set":
      return { ...state, copilotList: action.value };
    case "scanAi/clear":
      return { ...state, aiNotes: {}, aiRisk: null };
    case "reportAi/clear":
      return { ...state, aiReportNote: null, aiReportBusy: false };
    default:
      return state;
  }
}
