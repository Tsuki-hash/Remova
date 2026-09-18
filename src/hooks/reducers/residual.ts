import type { CleanupItem, FullCleanupReport, IgnoreSuggestion } from "../../types";
import { defaultSelectable } from "../../lib/decision";

export type MonitorDiffState = {
  added_files: string[];
  added_reg_values: string[];
} | null;

export type ResidualState = {
  selectedPaths: Set<string>;
  evidence: string | null;
  ignoreSuggestions: IgnoreSuggestion[];
  lastReport: FullCleanupReport | null;
  monitoring: boolean;
  monitorDiff: MonitorDiffState;
  residualFromUninstall: boolean;
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
  | { type: "lastReport/set"; value: FullCleanupReport | null }
  | { type: "monitoring/set"; value: boolean }
  | { type: "monitorDiff/set"; value: MonitorDiffState }
  | { type: "residualFromUninstall/set"; value: boolean }
  | { type: "scanChrome/clear" };

export function initialResidualState(): ResidualState {
  return {
    selectedPaths: new Set(),
    evidence: null,
    ignoreSuggestions: [],
    lastReport: null,
    monitoring: false,
    monitorDiff: null,
    residualFromUninstall: false,
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
          action.items.filter(defaultSelectable).map((it) => it.path),
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
    case "lastReport/set":
      return { ...state, lastReport: action.value };
    case "monitoring/set":
      return { ...state, monitoring: action.value };
    case "monitorDiff/set":
      return { ...state, monitorDiff: action.value };
    case "residualFromUninstall/set":
      return { ...state, residualFromUninstall: action.value };
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
