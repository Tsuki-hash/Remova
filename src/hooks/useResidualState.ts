import { useCallback, useState } from "react";
import type { CleanupItem, FullCleanupReport, IgnoreSuggestion } from "../types";
import type { VerifyRow } from "../lib/api";
import { defaultSelectable } from "../lib/decision";

export type MonitorDiffState = {
  added_files: string[];
  added_reg_values: string[];
} | null;

/** Residual / ignore / monitor side state + actions (A-1 action-ized). */
export function useResidualState() {
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(new Set());
  const [evidence, setEvidence] = useState<string | null>(null);
  const [ignoreSuggestions, setIgnoreSuggestions] = useState<IgnoreSuggestion[]>([]);
  const [lastReport, setLastReport] = useState<FullCleanupReport | null>(null);
  const [monitoring, setMonitoring] = useState(false);
  const [monitorDiff, setMonitorDiff] = useState<MonitorDiffState>(null);
  const [residualFromUninstall, setResidualFromUninstall] = useState(false);

  const togglePath = useCallback((path: string) => {
    setSelectedPaths((s) => {
      const n = new Set(s);
      if (n.has(path)) n.delete(path);
      else n.add(path);
      return n;
    });
  }, []);

  const selectDefaultItems = useCallback((items: CleanupItem[]) => {
    setSelectedPaths(new Set(items.filter(defaultSelectable).map((it) => it.path)));
  }, []);

  const clearSelection = useCallback(() => setSelectedPaths(new Set()), []);

  const clearIgnoreSuggestions = useCallback(() => setIgnoreSuggestions([]), []);

  const clearResidualScan = useCallback(() => {
    setEvidence(null);
    setIgnoreSuggestions([]);
    setSelectedPaths(new Set());
  }, []);

  return {
    selectedPaths,
    setSelectedPaths,
    evidence,
    setEvidence,
    ignoreSuggestions,
    setIgnoreSuggestions,
    lastReport,
    setLastReport,
    monitoring,
    setMonitoring,
    monitorDiff,
    setMonitorDiff,
    residualFromUninstall,
    setResidualFromUninstall,
    actions: {
      togglePath,
      selectDefaultItems,
      clearSelection,
      clearIgnoreSuggestions,
      clearResidualScan,
    },
  };
}

/** List filter chrome (category / sort) — paired with useAppFilter. */
export function useListFilterChrome() {
  const [q, setQ] = useState("");
  const [sortCol, setSortCol] = useState<"name" | "size" | "recommend" | null>(null);
  const [sortDesc, setSortDesc] = useState(false);
  const [category, setCategoryState] = useState<"all" | "desktop" | "store" | "large" | "recent">(
    () => {
      const v = localStorage.getItem("remova_cat");
      return v === "desktop" || v === "store" || v === "large" || v === "recent" ? v : "all";
    },
  );

  const setQuery = useCallback((v: string) => setQ(v), []);

  return {
    q,
    setQ,
    sortCol,
    setSortCol,
    sortDesc,
    setSortDesc,
    category,
    setCategoryState,
    actions: { setQuery },
  };
}

export type { VerifyRow };
