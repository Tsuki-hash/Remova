import type {
  CleanupReport,
  FullCleanupReport,
  InstalledApp,
  ScanResult,
} from "../../types";
import { appKey } from "../../lib/appKey";

export type AppCoreState = {
  apps: InstalledApp[];
  loading: boolean;
  error: string | null;
  selected: InstalledApp | null;
  multi: Set<string>;
  scan: ScanResult | null;
  scanning: boolean;
  report: CleanupReport | FullCleanupReport | null;
  /**
   * Last completed *full* cleanup. Derived from `report` by the reducer so the two can
   * never describe different apps. Kept across `preview/close` on purpose: the More page can still
   * export/report the cleanup the user just performed.
   */
  lastReport: FullCleanupReport | null;
  useOfficial: boolean;
  admin: boolean | null;
  disk: string;
  uninstallingKey: string | null;
  ignorePub: string[];
  ignoreName: string[];
};

export type AppCoreAction =
  | { type: "apps/set"; value: InstalledApp[] }
  | { type: "loading/set"; value: boolean }
  | { type: "error/set"; value: string | null }
  | { type: "selected/set"; value: InstalledApp | null }
  | { type: "multi/toggle"; key: string }
  | { type: "multi/set"; value: Set<string> }
  | { type: "multi/update"; updater: (m: Set<string>) => Set<string> }
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

export function initialAppCoreState(): AppCoreState {
  return {
    apps: [],
    loading: true,
    error: null,
    selected: null,
    multi: new Set(),
    scan: null,
    scanning: false,
    report: null,
    lastReport: null,
    useOfficial: false,
    admin: null,
    disk: "",
    uninstallingKey: null,
    ignorePub: [],
    ignoreName: [],
  };
}

function isCompletedFullReport(
  value: CleanupReport | FullCleanupReport,
): value is FullCleanupReport {
  return "deleted" in value && !value.dry_run && !value.aborted;
}

export function appCoreReducer(state: AppCoreState, action: AppCoreAction): AppCoreState {
  switch (action.type) {
    case "apps/set": {
      // REV-FE-02: refresh must not leave ghost selection / multi keys for vanished rows.
      const apps = action.value;
      const live = new Set(apps.map(appKey));
      const multi = new Set([...state.multi].filter((k) => live.has(k)));
      const selected = (() => {
        if (!state.selected) return null;
        const key = appKey(state.selected);
        if (!live.has(key)) return null;
        return apps.find((a) => appKey(a) === key) ?? state.selected;
      })();
      return { ...state, apps, multi, selected };
    }
    case "loading/set":
      return { ...state, loading: action.value };
    case "error/set":
      return { ...state, error: action.value };
    case "selected/set":
      return { ...state, selected: action.value };
    case "multi/toggle": {
      const n = new Set(state.multi);
      if (n.has(action.key)) n.delete(action.key);
      else n.add(action.key);
      return { ...state, multi: n };
    }
    case "multi/set":
      return { ...state, multi: action.value };
    case "multi/update":
      return { ...state, multi: action.updater(state.multi) };
    case "scan/set":
      return { ...state, scan: action.value };
    case "scanning/set":
      return { ...state, scanning: action.value };
    case "report/set": {
      const value = action.value;
      // `lastReport` is derived here and nowhere else — one writer, no drift.
      const isFull = value !== null && isCompletedFullReport(value);
      return {
        ...state,
        report: value,
        lastReport: isFull ? value : state.lastReport,
      };
    }
    case "useOfficial/set":
      return { ...state, useOfficial: action.value };
    case "admin/set":
      return { ...state, admin: action.value };
    case "disk/set":
      return { ...state, disk: action.value };
    case "uninstallingKey/set":
      return { ...state, uninstallingKey: action.value };
    case "ignorePub/set":
      return { ...state, ignorePub: action.value };
    case "ignoreName/set":
      return { ...state, ignoreName: action.value };
    case "preview/close":
      return {
        ...state,
        scan: null,
        report: null,
        error: null,
      };
    default:
      return state;
  }
}
