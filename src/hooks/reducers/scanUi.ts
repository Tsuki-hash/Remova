import type { LinkedBucketId } from "../../lib/linkedItems";

export type ScanUiState = {
  kindFilter: LinkedBucketId | null;
  pendingBucket: LinkedBucketId | null;
  riskFilter: "confirm" | "keep" | null;
  aiSummaryNote: string | null;
  aiNudgeDismissed: boolean;
};

export type ScanUiAction =
  | { type: "kindFilter/set"; value: LinkedBucketId | null }
  | { type: "pendingBucket/set"; value: LinkedBucketId | null }
  | { type: "pendingBucket/take" }
  | { type: "riskFilter/set"; value: "confirm" | "keep" | null }
  | { type: "riskFilter/toggle"; mode: "confirm" | "keep" }
  | { type: "aiSummary/set"; value: string | null }
  | { type: "aiNudge/dismiss" }
  | { type: "chrome/clear" };

export function initialScanUiState(): ScanUiState {
  return {
    kindFilter: null,
    pendingBucket: null,
    riskFilter: null,
    aiSummaryNote: null,
    aiNudgeDismissed: localStorage.getItem("remova_ai_nudge") === "1",
  };
}

export function scanUiReducer(state: ScanUiState, action: ScanUiAction): ScanUiState {
  switch (action.type) {
    case "kindFilter/set":
      return { ...state, kindFilter: action.value };
    case "pendingBucket/set":
      return { ...state, pendingBucket: action.value };
    case "pendingBucket/take": {
      const v = state.pendingBucket;
      return { ...state, pendingBucket: null, kindFilter: v ?? state.kindFilter };
    }
    case "riskFilter/set":
      return { ...state, riskFilter: action.value };
    case "riskFilter/toggle":
      return {
        ...state,
        kindFilter: null,
        riskFilter: state.riskFilter === action.mode ? null : action.mode,
      };
    case "aiSummary/set":
      return { ...state, aiSummaryNote: action.value };
    case "aiNudge/dismiss":
      return { ...state, aiNudgeDismissed: true };
    case "chrome/clear":
      return {
        ...state,
        kindFilter: null,
        riskFilter: null,
        aiSummaryNote: null,
        pendingBucket: null,
      };
    default:
      return state;
  }
}
