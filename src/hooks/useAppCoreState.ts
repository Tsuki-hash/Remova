import { useCallback, useReducer } from "react";
import type { InstalledApp } from "../types";
import type { VerifyRow } from "../lib/api";

export type { VerifyRow };

/** App core domain state (apps/list selection/scan/report) via reducer. */
export function useAppCoreState() {
  const [state, dispatch] = useReducer(
    (prev: CoreState, action: CoreAction): CoreState => {
      switch (action.type) {
        case "apps/set":
          return { ...prev, apps: action.value };
        case "loading/set":
          return { ...prev, loading: action.value };
        case "error/set":
          return { ...prev, error: action.value };
        case "selected/set":
          return { ...prev, selected: action.value };
        case "multi/toggle": {
          const n = new Set(prev.multi);
          if (n.has(action.key)) n.delete(action.key);
          else n.add(action.key);
          return { ...prev, multi: n };
        }
        case "multi/set":
          return { ...prev, multi: action.value };
        case "multi/removeKeys": {
          const okSet = new Set(action.keys);
          const n = new Set(prev.multi);
          for (const k of okSet) n.delete(k);
          return { ...prev, multi: n };
        }
        case "multi/clear":
          return { ...prev, multi: new Set() };
        case "scan/set":
          return { ...prev, scan: action.value };
        case "scanning/set":
          return { ...prev, scanning: action.value };
        case "report/set":
          return { ...prev, report: action.value };
        case "useOfficial/set":
          return { ...prev, useOfficial: action.value };
        case "admin/set":
          return { ...prev, admin: action.value };
        case "disk/set":
          return { ...prev, disk: action.value };
        case "uninstallingKey/set":
          return { ...prev, uninstallingKey: action.value };
        case "ignorePub/set":
          return { ...prev, ignorePub: action.value };
        case "ignoreName/set":
          return { ...prev, ignoreName: action.value };
        case "preview/close":
          return { ...prev, scan: null, report: null, error: null };
        default:
          return prev;
      }
    },
    undefined,
    initialCore,
  );

  const setApps = useCallback((value: InstalledApp[]) => dispatch({ type: "apps/set", value }), []);
  const setLoading = useCallback((value: boolean) => dispatch({ type: "loading/set", value }), []);
  const setError = useCallback((value: string | null) => dispatch({ type: "error/set", value }), []);
  const setSelected = useCallback(
    (value: InstalledApp | null) => dispatch({ type: "selected/set", value }),
    [],
  );
  const toggleMulti = useCallback((key: string) => dispatch({ type: "multi/toggle", key }), []);
  const setMulti = useCallback(
    (updater: Set<string> | ((m: Set<string>) => Set<string>)) => {
      if (typeof updater === "function") dispatch({ type: "multi/clear" });
      else dispatch({ type: "multi/set", value: updater });
    },
    [],
  );
  const removeDoneKeys = useCallback((keys: string[]) => {
    dispatch({ type: "multi/removeKeys", keys });
  }, []);
  const clearMulti = useCallback(() => dispatch({ type: "multi/clear" }), []);
  const setScan = useCallback((value: CoreState["scan"]) => dispatch({ type: "scan/set", value }), []);
  const setScanning = useCallback((value: boolean) => dispatch({ type: "scanning/set", value }), []);
  const setReport = useCallback(
    (value: CoreState["report"]) => dispatch({ type: "report/set", value }),
    [],
  );
  const setUseOfficial = useCallback(
    (value: boolean) => dispatch({ type: "useOfficial/set", value }),
    [],
  );
  const setAdmin = useCallback((value: boolean | null) => dispatch({ type: "admin/set", value }), []);
  const setDisk = useCallback((value: string) => dispatch({ type: "disk/set", value }), []);
  const setUninstallingKey = useCallback(
    (value: string | null) => dispatch({ type: "uninstallingKey/set", value }),
    [],
  );
  const setIgnorePub = useCallback((value: string[]) => dispatch({ type: "ignorePub/set", value }), []);
  const setIgnoreName = useCallback((value: string[]) => dispatch({ type: "ignoreName/set", value }), []);
  const closePreviewCore = useCallback(() => dispatch({ type: "preview/close" }), []);

  return {
    ...state,
    setApps,
    setLoading,
    setError,
    setSelected,
    toggleMulti,
    setMulti,
    removeDoneKeys,
    clearMulti,
    setScan,
    setScanning,
    setReport,
    setUseOfficial,
    setAdmin,
    setDisk,
    setUninstallingKey,
    setIgnorePub,
    setIgnoreName,
    closePreviewCore,
  };
}

import type { CleanupReport, FullCleanupReport, ScanResult } from "../types";

type CoreState = {
  apps: InstalledApp[];
  loading: boolean;
  error: string | null;
  selected: InstalledApp | null;
  multi: Set<string>;
  scan: ScanResult | null;
  scanning: boolean;
  report: CleanupReport | FullCleanupReport | null;
  useOfficial: boolean;
  admin: boolean | null;
  disk: string;
  uninstallingKey: string | null;
  ignorePub: string[];
  ignoreName: string[];
};

type CoreAction =
  | { type: "apps/set"; value: InstalledApp[] }
  | { type: "loading/set"; value: boolean }
  | { type: "error/set"; value: string | null }
  | { type: "selected/set"; value: InstalledApp | null }
  | { type: "multi/toggle"; key: string }
  | { type: "multi/set"; value: Set<string> }
  | { type: "multi/removeKeys"; keys: string[] }
  | { type: "multi/clear" }
  | { type: "scan/set"; value: ScanResult | null }
  | { type: "scanning/set"; value: boolean }
  | { type: "report/set"; value: CleanupReport | FullCleanupReport | null }
  | { type: "useOfficial/set"; value: boolean }
  | { type: "admin/set"; value: boolean | null }
  | { type: "disk/set"; value: string }
  | { type: "uninstallingKey/set"; value: string | null }
  | { type: "ignorePub/set"; value: string[] }
  | { type: "ignoreName/set"; value: string[] }
  | { type: "preview/close" };

function initialCore(): CoreState {
  return {
    apps: [],
    loading: true,
    error: null,
    selected: null,
    multi: new Set(),
    scan: null,
    scanning: false,
    report: null,
    useOfficial: false,
    admin: null,
    disk: "",
    uninstallingKey: null,
    ignorePub: [],
    ignoreName: [],
  };
}
