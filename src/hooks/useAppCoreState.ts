import { useCallback, useMemo, useReducer, useRef } from "react";
import type {
  CleanupReport,
  FullCleanupReport,
  InstalledApp,
  ScanResult,
} from "../types";
import type { VerifyRow } from "../lib/api";
import { appCoreReducer, initialAppCoreState } from "./reducers/appCore";

export type { VerifyRow };

/** App core domain state via shared appCore reducer (single source, A-01). */
export function useAppCoreState() {
  const [state, dispatch] = useReducer(appCoreReducer, undefined, initialAppCoreState);
  const stateRef = useRef(state);
  stateRef.current = state;

  const setApps = useCallback((value: InstalledApp[]) => dispatch({ type: "apps/set", value }), []);
  const setLoading = useCallback((value: boolean) => dispatch({ type: "loading/set", value }), []);
  const setError = useCallback((value: string | null) => dispatch({ type: "error/set", value }), []);
  const setSelected = useCallback(
    (value: InstalledApp | null) => dispatch({ type: "selected/set", value }),
    [],
  );
  const toggleMulti = useCallback((key: string) => dispatch({ type: "multi/toggle", key }), []);
  /** Supports React functional updaters inside the reducer (FE-N1). */
  const setMulti = useCallback(
    (updater: Set<string> | ((m: Set<string>) => Set<string>)) => {
      if (typeof updater === "function") {
        dispatch({ type: "multi/update", updater });
      } else {
        dispatch({ type: "multi/set", value: updater });
      }
    },
    [],
  );
  const removeDoneKeys = useCallback((keys: string[]) => {
    dispatch({ type: "multi/removeKeys", keys });
  }, []);
  const clearMulti = useCallback(() => dispatch({ type: "multi/clear" }), []);
  const setScan = useCallback(
    (value: ScanResult | null) => dispatch({ type: "scan/set", value }),
    [],
  );
  const setScanning = useCallback((value: boolean) => dispatch({ type: "scanning/set", value }), []);
  const setReport = useCallback(
    (value: CleanupReport | FullCleanupReport | null) => dispatch({ type: "report/set", value }),
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

  return useMemo(
    () => ({
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
    }),
    [
      state,
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
    ],
  );
}
