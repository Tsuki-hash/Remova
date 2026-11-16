import { cleanupItemKey } from "../../lib/cleanupPlan";
import type { CleanupItem, IgnoreSuggestion } from "../../types";
import { defaultSelectable } from "../../lib/decision";
import type { UninstallReview } from "../../lib/uninstallReview";

export type MonitorDiffState = {
  added_files: string[];
  added_reg_values: string[];
  /** Server-computed cleanup items from `end_install_monitor` (trusted). */
  items?: CleanupItem[];
  scope_revision?: number;
} | null;

export type ResidualState = {
  selectedPaths: Set<string>;
  evidence: string | null;
  ignoreSuggestions: IgnoreSuggestion[];
  monitoring: boolean;
  monitorDiff: MonitorDiffState;
  uninstallReview: UninstallReview | null;
};

export type ResidualAction =
  | { type: "selection/toggle"; path: string }
  | { type: "selection/set"; paths: Set<string> }
  | { type: "selection/defaultItems"; items: CleanupItem[] }
  | { type: "selection/update"; updater: (s: Set<string>) => Set<string> }
  | { type: "selection/clear" }
  | { type: "evidence/set"; value: string | null }
  | { type: "ignore/set"; value: IgnoreSuggestion[] }
  | { type: "ignore/clear" }
  | { type: "monitoring/set"; value: boolean }
  | { type: "monitorDiff/set"; value: MonitorDiffState }
  | { type: "uninstallReview/set"; value: UninstallReview | null }
  | { type: "scanChrome/clear" };

export function initialResidualState(): ResidualState {
  return {
    selectedPaths: new Set(),
    evidence: null,
    ignoreSuggestions: [],
    monitoring: false,
    monitorDiff: null,
    uninstallReview: null,
  };
}

export function residualReducer(state: ResidualState, action: ResidualAction): ResidualState {
  switch (action.type) {
    case "selection/toggle": {
      const n = new Set(state.selectedPaths);
      if (n.has(action.path)) n.delete(action.path);
      else n.add(action.path);
      return { ...state, selectedPaths: n };
    }
    case "selection/set":
      return { ...state, selectedPaths: action.paths };
    case "selection/defaultItems":
      return {
        ...state,
        selectedPaths: new Set(
          action.items.filter(defaultSelectable).map(cleanupItemKey),
        ),
      };
    case "selection/update":
      return { ...state, selectedPaths: action.updater(state.selectedPaths) };
    case "selection/clear":
      return { ...state, selectedPaths: new Set() };
    case "evidence/set":
      return { ...state, evidence: action.value };
    case "ignore/set":
      return { ...state, ignoreSuggestions: action.value };
    case "ignore/clear":
      return { ...state, ignoreSuggestions: [] };
    case "monitoring/set":
      return { ...state, monitoring: action.value };
    case "monitorDiff/set":
      return { ...state, monitorDiff: action.value };
    case "uninstallReview/set":
      return { ...state, uninstallReview: action.value };
    case "scanChrome/clear":
      return {
        ...state,
        evidence: null,
        ignoreSuggestions: [],
        selectedPaths: new Set(),
      };
    default:
      return state;
  }
}
