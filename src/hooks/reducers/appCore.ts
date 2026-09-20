import type {
  CleanupReport,
  FullCleanupReport,
  InstalledApp,
  ScanResult,
} from "../../types";

export type AppCoreState = {
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

export type AppCoreAction =
  | { type: "apps/set"; value: InstalledApp[] }
  | { type: "loading/set"; value: boolean }
  | { type: "error/set"; value: string | null }
  | { type: "selected/set"; value: InstalledApp | null }
  | { type: "multi/toggle"; key: string }
  | { type: "multi/set"; value: Set<string> }
  | { type: "multi/update"; updater: (m: Set<string>) => Set<string> }
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
    useOfficial: false,
    admin: null,
    disk: "",
    uninstallingKey: null,
    ignorePub: [],
    ignoreName: [],
  };
}

export function appCoreReducer(state: AppCoreState, action: AppCoreAction): AppCoreState {
  switch (action.type) {
    case "apps/set":
      return { ...state, apps: action.value };
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
    case "multi/removeKeys": {
      const okSet = new Set(action.keys);
      const n = new Set(state.multi);
      for (const k of okSet) n.delete(k);
      return { ...state, multi: n };
    }
    case "multi/clear":
      return { ...state, multi: new Set() };
    case "scan/set":
      return { ...state, scan: action.value };
    case "scanning/set":
      return { ...state, scanning: action.value };
    case "report/set":
      return { ...state, report: action.value };
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
