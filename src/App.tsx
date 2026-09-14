import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  CleanupReport,
  FullCleanupReport,
  InstalledApp,
  ScanResult,
} from "./types";
import { currentLang, formatSize, loadLang, setLang, t } from "./i18n";

type Theme = "light" | "dark";

/** Process-wide icon cache: displayIcon raw → data URL (or null on failure). */
const iconCache = new Map<string, string | null>();
const iconInflight = new Map<string, Promise<string | null>>();

function loadAppIcon(displayIcon: string): Promise<string | null> {
  if (iconCache.has(displayIcon)) {
    return Promise.resolve(iconCache.get(displayIcon) ?? null);
  }
  const existing = iconInflight.get(displayIcon);
  if (existing) return existing;
  const p = invoke<string | null>("app_icon_data", { displayIcon })
    .then((url) => {
      iconCache.set(displayIcon, url);
      iconInflight.delete(displayIcon);
      return url;
    })
    .catch(() => {
      iconCache.set(displayIcon, null);
      iconInflight.delete(displayIcon);
      return null;
    });
  iconInflight.set(displayIcon, p);
  return p;
}

function AppIcon({ displayIcon, name }: { displayIcon: string; name: string }) {
  const [src, setSrc] = useState<string | null>(() =>
    displayIcon ? (iconCache.get(displayIcon) ?? null) : null,
  );

  useEffect(() => {
    if (!displayIcon) {
      setSrc(null);
      return;
    }
    let cancelled = false;
    void loadAppIcon(displayIcon).then((url) => {
      if (!cancelled) setSrc(url);
    });
    return () => {
      cancelled = true;
    };
  }, [displayIcon]);

  const initial = (name.trim()[0] || "?").toUpperCase();
  return (
    <span
      style={{
        width: 20,
        height: 20,
        borderRadius: 4,
        flexShrink: 0,
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        background: "var(--th-bg)",
        color: "var(--muted)",
        fontSize: 11,
        fontWeight: 600,
        overflow: "hidden",
      }}
      aria-hidden
    >
      {src ? (
        <img src={src} width={20} height={20} alt="" style={{ display: "block" }} />
      ) : (
        initial
      )}
    </span>
  );
}

function loadTheme(): Theme {
  const v = localStorage.getItem("remova_theme");
  return v === "dark" ? "dark" : "light";
}

function applyTheme(theme: Theme) {
  const root = document.documentElement;
  if (theme === "dark") {
    root.style.setProperty("--bg", "#0b0f14");
    root.style.setProperty("--fg", "#e7eef7");
    root.style.setProperty("--muted", "#8b9bb0");
    root.style.setProperty("--surface", "#141a22");
    root.style.setProperty("--surface-2", "#1b2330");
    root.style.setProperty("--border", "#2a3444");
    root.style.setProperty("--accent", "#2dd4bf");
    root.style.setProperty("--accent-ink", "#042f2e");
    root.style.setProperty("--danger", "#f87171");
    root.style.setProperty("--th-bg", "#1b2330");
    root.style.setProperty("--row-hover", "#1f2937");
    root.style.setProperty("--shadow", "0 8px 24px rgba(0,0,0,.35)");
  } else {
    root.style.setProperty("--bg", "#f3f6f9");
    root.style.setProperty("--fg", "#0f172a");
    root.style.setProperty("--muted", "#5b6b7f");
    root.style.setProperty("--surface", "#ffffff");
    root.style.setProperty("--surface-2", "#f8fafc");
    root.style.setProperty("--border", "#d8e0ea");
    root.style.setProperty("--accent", "#0f766e");
    root.style.setProperty("--accent-ink", "#ffffff");
    root.style.setProperty("--danger", "#dc2626");
    root.style.setProperty("--th-bg", "#f1f5f9");
    root.style.setProperty("--row-hover", "#f0fdfa");
    root.style.setProperty("--shadow", "0 10px 30px rgba(15,23,42,.06)");
  }
  localStorage.setItem("remova_theme", theme);
}

/** Certificate DN or noisy package name → short readable label. */
function prettyPublisher(raw: string): string {
  const s = (raw || "").trim();
  if (!s || s === "—") return "—";
  if (/^CN=/i.test(s) || s.includes(", O=") || s.includes(",OU=")) {
    const cn = /CN=([^,]+)/i.exec(s)?.[1]?.trim();
    if (cn && !/^[0-9a-f-]{20,}$/i.test(cn) && cn.length <= 48) return cn;
    if (cn && /^[0-9a-f-]{20,}$/i.test(cn)) return "Signed package";
    return "Signed package";
  }
  return s.length > 42 ? `${s.slice(0, 40)}…` : s;
}

function prettyAppName(name: string, source: string): string {
  const n = (name || "").trim();
  if (!n) return "—";
  // Store packages sometimes use long hex / GUID-like ids as Name.
  if (source === "Store" && /^[0-9a-f]{6,}(\.[0-9a-f]+)+$/i.test(n)) {
    return `Store app · ${n.slice(0, 8)}…`;
  }
  return n;
}

function shortPath(p: string): string {
  const s = p || "";
  if (s.length <= 64) return s;
  return `${s.slice(0, 28)}…${s.slice(-28)}`;
}

type ErrorContext = "analyze" | "cleanup" | "elevate" | "invoke";

/** Map backend error tokens / raw exceptions to user-actionable copy. */
function formatError(e: unknown, ctx: ErrorContext = "invoke"): string {
  const L = t();
  const raw = typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
  const elev = raw.match(/^elevate:(denied|cancelled|not_found|failed):(\d+)/);
  if (elev) {
    const kind = elev[1];
    const code = Number(elev[2]);
    if (kind === "denied") return L.errElevateDenied;
    if (kind === "cancelled") return L.errElevateCancelled;
    if (kind === "not_found") return L.errElevateNotFound;
    return L.errElevateFailed(code);
  }
  const detail = raw.replace(/^Error:\s*/i, "").trim() || raw;
  if (ctx === "analyze") return L.errAnalyzeFailed(detail);
  if (ctx === "cleanup") return L.errCleanupFailed(detail);
  if (ctx === "elevate") return L.errElevateFailed(0);
  return L.errInvokeFailed(detail);
}

function escapeHtml(s: string): string {
  return String(s ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

export default function App() {
  const [apps, setApps] = useState<InstalledApp[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const [selected, setSelected] = useState<InstalledApp | null>(null);
  const [multi, setMulti] = useState<Set<string>>(new Set());
  const [scan, setScan] = useState<ScanResult | null>(null);
  const [scanning, setScanning] = useState(false);
  const [report, setReport] = useState<CleanupReport | FullCleanupReport | null>(null);
  const [dryRunning, setDryRunning] = useState(false);
  const [useOfficial, setUseOfficial] = useState(false);
  const [sortCol, setSortCol] = useState<
    "name" | "publisher" | "install_location" | "size" | "install_date" | null
  >(null);
  const [sortDesc, setSortDesc] = useState(false);
  const [admin, setAdmin] = useState<boolean | null>(null);
  const [disk, setDisk] = useState("");
  const [theme, setTheme] = useState<Theme>(loadTheme());
  const [langVer, setLangVer] = useState(0);
  const [showGuide, setShowGuide] = useState(!localStorage.getItem("remova_guided"));
  const [notice, setNotice] = useState<string | null>(null);
  const [showHistory, setShowHistory] = useState(false);
  const [history, setHistory] = useState<
    { app_name: string; deleted: number; failed: number; backup_dir: string }[]
  >([]);
  const [batching, setBatching] = useState(false);
  type BatchStatus = "ok" | "failed" | "skipped";
  type BatchItemResult = {
    key: string;
    name: string;
    status: BatchStatus;
    detail: string;
  };
  const [batchIndex, setBatchIndex] = useState(0);
  const [batchTotal, setBatchTotal] = useState(0);
  const [batchCurrent, setBatchCurrent] = useState("");
  const [batchResults, setBatchResults] = useState<BatchItemResult[]>([]);
  const [showBatchSummary, setShowBatchSummary] = useState(false);
  const batchCancelRef = useRef(false);
  const [showRestore, setShowRestore] = useState(false);
  const [restoreSessions, setRestoreSessions] = useState<string[]>([]);
  const [restorePick, setRestorePick] = useState("");
  const [restoreBusy, setRestoreBusy] = useState(false);
  const [restoreMsgs, setRestoreMsgs] = useState<string[]>([]);
  type ManageTab = "startup" | "services" | "tasks";
  type ManageItem = {
    name: string;
    detail: string;
    location: string;
    enabled: boolean;
  };
  const [showManage, setShowManage] = useState(false);
  const [manageTab, setManageTab] = useState<ManageTab>("startup");
  const [manageItems, setManageItems] = useState<ManageItem[]>([]);
  const [manageBusy, setManageBusy] = useState(false);
  const [forceBusy, setForceBusy] = useState(false);
  const [shellMenu, setShellMenu] = useState(false);
  const [showTools, setShowTools] = useState(false);
  const [ignorePub, setIgnorePub] = useState<string[]>([]);
  const [ignoreName, setIgnoreName] = useState<string[]>([]);
  const [monitoring, setMonitoring] = useState(false);
  const [monitorDiff, setMonitorDiff] = useState<{
    added_files: string[];
    added_reg_values: string[];
  } | null>(null);
  const [lastReport, setLastReport] = useState<FullCleanupReport | null>(null);
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(new Set());
  const [evidence, setEvidence] = useState<string | null>(null);
  const [histQ, setHistQ] = useState("");
  const busyRef = useRef(false);
  const [sizeMap, setSizeMap] = useState<Record<string, number>>({});
  const [estimating, setEstimating] = useState(false);
  const sizeCache = useRef<Map<string, number>>(new Map());
  const sizeCancelRef = useRef(false);

  const sizeOf = useCallback((a: InstalledApp): number => {
    if (a.estimated_size_kb > 0) return a.estimated_size_kb;
    return sizeCache.current.get(a.install_location) ?? 0;
  }, []);

  const formatAppSize = useCallback(
    (a: InstalledApp): string => {
      if (a.estimated_size_kb > 0) return formatSize(a.estimated_size_kb);
      const est = sizeCache.current.get(a.install_location) ?? 0;
      if (est > 0) return `~${formatSize(est)}`;
      return "—";
    },
    [],
  );

  useEffect(() => {
    loadLang();
    applyTheme(theme);
    setLangVer((v) => v + 1);
  }, [theme]);

  useEffect(() => {
    const onBeforeUnload = (e: BeforeUnloadEvent) => {
      if (batching || dryRunning) {
        e.preventDefault();
        e.returnValue = "";
      }
    };
    window.addEventListener("beforeunload", onBeforeUnload);
    return () => window.removeEventListener("beforeunload", onBeforeUnload);
  }, [batching, dryRunning]);

  const analyze = useCallback(async (app: InstalledApp) => {
    setSelected(app);
    setScanning(true);
    setScan(null);
    setReport(null);
    const t0 = performance.now();
    try {
      const r = await invoke<ScanResult>("analyze_associations", { app });
      setScan(r);
      // default select confirmed non-high
      setSelectedPaths(
        new Set(
          r.items
            .filter((it) => it.confidence === "confirmed" && it.risk !== "high")
            .map((it) => it.path),
        ),
      );
      setError(null);
      setNotice(`analyze ${((performance.now() - t0) / 1000).toFixed(1)}s · ${r.items.length} items`);
    } catch (e) {
      setError(formatError(e, "analyze"));
    } finally {
      setScanning(false);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const list = await invoke<InstalledApp[]>("list_installed_apps");
        if (!cancelled) setApps(list);
      } catch (e) {
        if (!cancelled) setError(formatError(e));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    void invoke<boolean>("is_elevated").then(setAdmin).catch(() => {});
    void invoke<{ publishers: string[]; names: string[]; paths: string[] }>("load_ignore")
      .then((ig) => {
        setIgnorePub(ig.publishers || []);
        setIgnoreName(ig.names || []);
      })
      .catch(() => {});
    void invoke<{ free_gb: number; total_gb: number }>("disk_usage")
      .then((d) => setDisk(`C: ${d.free_gb.toFixed(1)} / ${d.total_gb.toFixed(0)} GB`))
      .catch(() => {});
    void fetch("https://api.github.com/repos/Tsuki-hash/Remova/releases/latest", {
      headers: { Accept: "application/vnd.github+json" },
    })
      .then((r) => (r.ok ? r.json() : null))
      .then((j: { tag_name?: string } | null) => {
        if (j?.tag_name) setNotice(`${t().versionNew}: ${j.tag_name}`);
      })
      .catch(() => {});
    // tauri window close confirm
    void (async () => {
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        const win = getCurrentWindow();
        await win.onCloseRequested(async (event) => {
          if (busyRef.current) {
            if (
              !window.confirm(
                "Task in progress. Close window may interrupt cleanup. Close anyway?",
              )
            ) {
              event.preventDefault();
            }
          }
        });
      } catch {
        // not in tauri
      }
    })();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Auto-estimate missing sizes after list load (P0-2).
  useEffect(() => {
    if (loading || apps.length === 0) return;
    const pending = [
      ...new Set(
        apps
          .filter((a) => !(a.estimated_size_kb > 0) && a.install_location?.trim())
          .map((a) => a.install_location.trim()),
      ),
    ].filter((p) => !sizeCache.current.has(p));
    if (pending.length === 0) return;

    let disposed = false;
    sizeCancelRef.current = false;
    setEstimating(true);
    void invoke("begin_size_estimate").catch(() => {});

    (async () => {
      const workers = Array.from({ length: 2 }, async () => {
        while (pending.length > 0 && !disposed && !sizeCancelRef.current) {
          const path = pending.shift();
          if (!path) break;
          try {
            const kb = await invoke<number>("estimate_dir_size_kb", { path });
            if (disposed || sizeCancelRef.current) break;
            sizeCache.current.set(path, kb > 0 ? kb : 0);
            setSizeMap((m) => ({ ...m, [path]: kb > 0 ? kb : 0 }));
          } catch {
            if (disposed || sizeCancelRef.current) break;
            sizeCache.current.set(path, 0);
            setSizeMap((m) => ({ ...m, [path]: 0 }));
          }
        }
      });
      await Promise.all(workers);
      if (!disposed) setEstimating(false);
    })();

    return () => {
      disposed = true;
    };
  }, [apps, loading]);

  const stopSizeEstimate = useCallback(async () => {
    sizeCancelRef.current = true;
    setEstimating(false);
    try {
      await invoke("cancel_size_estimate");
    } catch {
      // ignore
    }
  }, []);

  // Drag-drop: match dropped path to install_location and analyze (P1-3).
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      try {
        const { getCurrentWebview } = await import("@tauri-apps/api/webview");
        const webview = getCurrentWebview();
        const un = await webview.onDragDropEvent((event) => {
          if (event.payload.type !== "drop") return;
          const path = (event.payload.paths || [])[0];
          if (!path) return;
          const norm = path.replace(/\//g, "\\").toLowerCase();
          const hit =
            apps.find((a) => {
              const loc = (a.install_location || "").replace(/\//g, "\\").toLowerCase();
              return loc && (norm.startsWith(loc) || loc.startsWith(norm) || norm === loc);
            }) ?? null;
          if (hit) {
            setSelected(hit);
            void analyze(hit);
            setNotice(hit.name);
          } else {
            setNotice(`${t().dropHint}: ${path}`);
          }
        });
        if (cancelled) un();
        else unlisten = un;
      } catch {
        // not in tauri
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [apps]);

  const L = useMemo(() => t(), [langVer]);

  const filtered = useMemo(() => {
    const needle = q.trim().toLowerCase();
    let list = apps;
    list = list.filter(
      (a) =>
        !ignorePub.some((p) => p && a.publisher?.toLowerCase() === p.toLowerCase()) &&
        !ignoreName.some((n) => n && a.name?.toLowerCase() === n.toLowerCase()),
    );
    if (needle) {
      list = list.filter(
        (a) =>
          a.name.toLowerCase().includes(needle) ||
          a.publisher.toLowerCase().includes(needle) ||
          a.install_location.toLowerCase().includes(needle),
      );
    }
    if (!sortCol) return list;
    const s = [...list].sort((a, b) => {
      if (sortCol === "size") {
        return sizeOf(a) - sizeOf(b);
      }
      return (a[sortCol] || "").toLowerCase().localeCompare((b[sortCol] || "").toLowerCase());
    });
    return sortDesc ? s.reverse() : s;
  }, [apps, q, sortCol, sortDesc, sizeOf, sizeMap, ignorePub, ignoreName]);

  const sortBy = (col: "name" | "publisher" | "install_location" | "size" | "install_date") => {
    if (sortCol === col) setSortDesc((d) => !d);
    else {
      setSortCol(col);
      setSortDesc(col === "size" || col === "install_date");
    }
  };

  /** App used by 深度分析: row click, else the single multi-checkbox target. */
  const analyzeApp = useMemo(() => {
    if (selected) return selected;
    if (multi.size === 1) {
      const key = [...multi][0];
      return apps.find((a) => a.registry_key + a.name === key) ?? null;
    }
    return null;
  }, [selected, multi, apps]);

  const toggleMulti = (key: string) => {
    setMulti((m) => {
      const n = new Set(m);
      if (n.has(key)) n.delete(key);
      else n.add(key);
      return n;
    });
  };

  const openRestoreSessions = useCallback(async () => {
    setShowRestore(true);
    setRestoreMsgs([]);
    setRestorePick("");
    setRestoreSessions([]);
    try {
      const names = await invoke<string[]>("list_restore_sessions");
      setRestoreSessions(names);
      if (names.length > 0) setRestorePick(names[0]);
    } catch (e) {
      setError(formatError(e));
      setShowRestore(false);
    }
  }, []);

  const runRestoreSession = useCallback(async () => {
    if (!restorePick || restoreBusy) return;
    if (!window.confirm(L.restoreConfirm)) return;
    setRestoreBusy(true);
    setRestoreMsgs([]);
    try {
      const msgs = await invoke<string[]>("restore_session_by_name", {
        name: restorePick,
      });
      setRestoreMsgs(msgs.length ? msgs : ["ok"]);
    } catch (e) {
      setRestoreMsgs([formatError(e)]);
    } finally {
      setRestoreBusy(false);
    }
  }, [restorePick, restoreBusy, L]);

  const loadManage = useCallback(async (tab: ManageTab) => {
    setManageTab(tab);
    setManageBusy(true);
    try {
      const cmd =
        tab === "startup"
          ? "list_startup_items"
          : tab === "services"
            ? "list_services"
            : "list_scheduled_tasks";
      const items = await invoke<ManageItem[]>(cmd);
      setManageItems(items);
    } catch (e) {
      setError(formatError(e));
    } finally {
      setManageBusy(false);
    }
  }, []);

  const toggleManageItem = useCallback(
    async (item: ManageItem) => {
      setManageBusy(true);
      try {
        if (manageTab === "startup") {
          await invoke("set_startup_enabled", {
            location: item.location,
            enabled: !item.enabled,
          });
        } else if (manageTab === "services") {
          await invoke("set_service_start_disabled", {
            name: item.name,
            disable: item.enabled,
          });
        } else {
          await invoke("set_task_enabled", {
            name: item.name,
            enabled: !item.enabled,
          });
        }
        await loadManage(manageTab);
      } catch (e) {
        setError(formatError(e));
      } finally {
        setManageBusy(false);
      }
    },
    [manageTab, loadManage],
  );

  const forceClean = useCallback(async () => {
    if (!selected || forceBusy) return;
    if (!window.confirm(L.forceCleanHint)) return;
    setForceBusy(true);
    busyRef.current = true;
    try {
      const r = await invoke<ScanResult>("analyze_associations", { app: selected });
      const items = r.items.filter(
        (it) => it.confidence === "confirmed" && it.risk !== "high",
      );
      if (!items.length) {
        setNotice(L.noHistory);
        return;
      }
      const report = await invoke<FullCleanupReport>("run_full_cleanup", {
        app: selected,
        items,
        options: {
          dry_run: false,
          skip_official_uninstall: true,
          backup_enabled: true,
        },
      });
      setNotice(
        `${L.forceClean}: ${selected.name} deleted=${report.deleted} failed=${report.failed}`,
      );
      setLastReport(report);
    } catch (e) {
      setError(formatError(e, "cleanup"));
    } finally {
      busyRef.current = false;
      setForceBusy(false);
    }
  }, [selected, forceBusy, L]);

  const doIgnorePublisher = useCallback(async () => {
    if (!selected?.publisher) return;
    try {
      const ig = await invoke<{ publishers: string[]; names: string[] }>("ignore_publisher", {
        name: selected.publisher,
      });
      setIgnorePub(ig.publishers || []);
      setNotice(L.ignoreLoaded);
    } catch (e) {
      setError(formatError(e));
    }
  }, [selected, L]);

  const doIgnoreApp = useCallback(async () => {
    if (!selected?.name) return;
    try {
      const ig = await invoke<{ publishers: string[]; names: string[] }>("ignore_app_name", {
        name: selected.name,
      });
      setIgnoreName(ig.names || []);
      setNotice(L.ignoreLoaded);
    } catch (e) {
      setError(formatError(e));
    }
  }, [selected, L]);

  const exportHtmlReport = useCallback(() => {
    if (!lastReport) return;
    const r = lastReport;
    const rows = (r.item_details || [])
      .map(
        (d) =>
          `<tr><td>${escapeHtml(d.kind)}</td><td>${escapeHtml(d.status)}</td><td>${escapeHtml(d.path)}</td><td>${escapeHtml(d.message)}</td></tr>`,
      )
      .join("\n");
    const html = `<!doctype html><html lang="zh"><head><meta charset="utf-8"><title>Remova report — ${escapeHtml(r.app_name)}</title>
<style>body{font:14px/1.5 system-ui,sans-serif;margin:24px;color:#111}table{border-collapse:collapse;width:100%}td,th{border:1px solid #ccc;padding:6px 8px;text-align:left;vertical-align:top}th{background:#eef}</style>
</head><body><h1>Remova — ${escapeHtml(r.app_name)}</h1>
<p>dry_run=${r.dry_run} deleted=${r.deleted} failed=${r.failed} skipped=${r.skipped} aborted=${r.aborted}</p>
<p>${escapeHtml(r.uninstall_message || "")}</p>
<p>backup: ${escapeHtml(r.backup_dir || "")}</p>
<table><tr><th>kind</th><th>status</th><th>path</th><th>message</th></tr>${rows}</table>
</body></html>`;
    const blob = new Blob([html], { type: "text/html;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `remova-report-${(r.app_name || "app").replace(/[^\w.-]+/g, "_")}.html`;
    a.click();
    URL.revokeObjectURL(url);
  }, [lastReport]);

  const runOrphanScan = useCallback(async () => {
    setNotice(L.orphanScanning);
    try {
      const items = await invoke<ScanResult["items"]>("scan_orphan_leftovers");
      setScan({
        app_name: L.orphanScan,
        items,
      });
      setSelectedPaths(new Set(items.filter((i) => i.confidence === "confirmed").map((i) => i.path)));
      setNotice(`${L.orphanScan}: ${items.length}`);
    } catch (e) {
      setError(formatError(e, "analyze"));
    }
  }, [L]);

  const toggleMonitor = useCallback(async () => {
    try {
      if (!monitoring) {
        await invoke("begin_install_monitor");
        setMonitoring(true);
        setMonitorDiff(null);
        setNotice(L.monitorRunning);
      } else {
        const d = await invoke<{ added_files: string[]; added_reg_values: string[] }>(
          "end_install_monitor",
        );
        setMonitoring(false);
        setMonitorDiff(d);
        setNotice(`${L.monitorDiff}: ${d.added_files.length} files / ${d.added_reg_values.length} reg`);
      }
    } catch (e) {
      setError(formatError(e));
      setMonitoring(false);
    }
  }, [monitoring, L]);

  const dryRun = useCallback(async () => {
    if (!scan) return;
    const items = scan.items.filter((it) => selectedPaths.has(it.path));
    setDryRunning(true);
    try {
      const r = await invoke<CleanupReport>("run_cleanup_dry_run", {
        appName: scan.app_name,
        items,
      });
      setReport(r);
    } catch (e) {
      setError(formatError(e, "cleanup"));
    } finally {
      setDryRunning(false);
    }
  }, [scan, selectedPaths]);

  const execReal = useCallback(async () => {
    if (!scan || !selected) return;
    const items = scan.items.filter((it) => selectedPaths.has(it.path));
    busyRef.current = true;
    setDryRunning(true);
    try {
      const r = await invoke<FullCleanupReport>("run_full_cleanup", {
        app: selected,
        items,
        options: {
          dry_run: false,
          skip_official_uninstall: !useOfficial,
          backup_enabled: true,
        },
      });
      setReport(r);
      if (r && typeof r === "object" && "deleted" in r) {
        setLastReport(r as FullCleanupReport);
      }
    } catch (e) {
      setError(formatError(e, "cleanup"));
    } finally {
      busyRef.current = false;
      setDryRunning(false);
    }
  }, [scan, selected, useOfficial, selectedPaths]);

  const batchCleanup = useCallback(async () => {
    const keys = new Set(multi);
    const queue = apps.filter((a) => keys.has(a.registry_key + a.name));
    if (!queue.length) {
      setNotice(L.selectRowHint);
      return;
    }
    if (!window.confirm(L.batchConfirm(queue.length))) return;

    busyRef.current = true;
    batchCancelRef.current = false;
    setBatching(true);
    setBatchIndex(0);
    setBatchTotal(queue.length);
    setBatchResults([]);
    setShowBatchSummary(false);
    const results: BatchItemResult[] = [];
    const okKeys = new Set<string>();
    try {
      for (let i = 0; i < queue.length; i++) {
        if (batchCancelRef.current) break;
        const app = queue[i];
        const key = app.registry_key + app.name;
        setBatchIndex(i + 1);
        setBatchCurrent(app.name);
        setNotice(`[${i + 1}/${queue.length}] ${app.name}`);
        try {
          const r = await invoke<ScanResult>("analyze_associations", { app });
          const items = r.items.filter(
            (it) => it.confidence === "confirmed" && it.risk !== "high",
          );
          if (!items.length) {
            results.push({
              key,
              name: app.name,
              status: "skipped",
              detail: "",
            });
            okKeys.add(key);
            continue;
          }
          const report = await invoke<FullCleanupReport>("run_full_cleanup", {
            app,
            items,
            options: {
              dry_run: false,
              skip_official_uninstall: true,
              backup_enabled: true,
            },
          });
          const detail = `deleted=${report.deleted} failed=${report.failed}`;
          if (report.failed > 0 && report.deleted === 0) {
            results.push({ key, name: app.name, status: "failed", detail });
          } else {
            results.push({ key, name: app.name, status: "ok", detail });
            okKeys.add(key);
          }
        } catch (e) {
          results.push({
            key,
            name: app.name,
            status: "failed",
            detail: formatError(e, "cleanup"),
          });
        }
        setBatchResults([...results]);
      }
      const cancelled = batchCancelRef.current;
      setNotice(cancelled ? L.batchCancelled : L.batchDone);
      setMulti((m) => {
        const n = new Set(m);
        for (const k of okKeys) n.delete(k);
        return n;
      });
      setShowBatchSummary(true);
    } catch (e) {
      setError(formatError(e, "cleanup"));
    } finally {
      busyRef.current = false;
      setBatching(false);
      setBatchCurrent("");
      setBatchIndex(0);
    }
  }, [apps, multi, L]);

  const cancelBatch = useCallback(() => {
    batchCancelRef.current = true;
    setNotice(L.batchCancelHint);
  }, [L]);

  const retryFailedBatch = useCallback(() => {
    const failed = batchResults.filter((r) => r.status === "failed");
    if (!failed.length) return;
    setMulti(new Set(failed.map((r) => r.key)));
    setBatchResults([]);
    setShowBatchSummary(false);
  }, [batchResults]);

  const css = {
    page: {
      padding: "20px 24px 32px",
      maxWidth: 1400,
      margin: "0 auto",
      color: "var(--fg)",
      fontFamily:
        "'Segoe UI', 'PingFang SC', 'Microsoft YaHei UI', system-ui, sans-serif",
    },
    muted: { color: "var(--muted)", fontSize: 13 },
    input: {
      flex: 1,
      minWidth: 200,
      height: 40,
      borderRadius: 10,
      border: "1px solid var(--border)",
      padding: "0 14px",
      fontSize: 14,
      background: "var(--surface)",
      color: "var(--fg)",
      outline: "none",
    },
    btn: {
      height: 36,
      padding: "0 14px",
      borderRadius: 10,
      border: "none",
      background: "var(--accent)",
      color: "var(--accent-ink)",
      fontWeight: 600,
      fontSize: 13,
      cursor: "pointer",
      whiteSpace: "nowrap" as const,
      transition: "opacity .15s, transform .05s",
    },
    btnGhost: {
      height: 36,
      padding: "0 12px",
      borderRadius: 10,
      border: "1px solid var(--border)",
      background: "var(--surface)",
      color: "var(--fg)",
      fontSize: 13,
      cursor: "pointer",
      whiteSpace: "nowrap" as const,
      transition: "background .15s, border-color .15s, opacity .15s",
    },
    btnSm: {
      height: 30,
      padding: "0 10px",
      borderRadius: 8,
      border: "1px solid var(--border)",
      background: "var(--surface)",
      color: "var(--fg)",
      fontSize: 12,
      cursor: "pointer",
      whiteSpace: "nowrap" as const,
    },
    card: {
      background: "var(--surface)",
      border: "1px solid var(--border)",
      borderRadius: 14,
      overflow: "hidden",
      boxShadow: "var(--shadow)",
    },
    th: {
      textAlign: "left" as const,
      padding: "10px 12px",
      background: "var(--th-bg)",
      borderBottom: "1px solid var(--border)",
      position: "sticky" as const,
      top: 0,
      zIndex: 1,
      fontSize: 12,
      fontWeight: 600,
      color: "var(--muted)",
      letterSpacing: 0.2,
    },
    td: {
      padding: "10px 12px",
      borderBottom: "1px solid var(--border)",
      verticalAlign: "middle" as const,
      fontSize: 13,
    },
    table: { width: "100%", borderCollapse: "collapse" as const, fontSize: 13 },
    scroll: {
      maxHeight: "calc(100vh - 280px)",
      overflow: "auto" as const,
      overscrollBehavior: "contain" as const,
    },
    toolbar: {
      display: "flex",
      gap: 8,
      alignItems: "center",
      flexWrap: "wrap" as const,
      marginBottom: 10,
    },
    chip: {
      display: "inline-flex",
      alignItems: "center",
      gap: 6,
      height: 28,
      padding: "0 10px",
      borderRadius: 999,
      background: "var(--surface-2)",
      border: "1px solid var(--border)",
      fontSize: 12,
      color: "var(--muted)",
    },
  };

  const globalCss = `
    button:disabled { opacity: .45; cursor: not-allowed; }
    button:not(:disabled):hover { filter: brightness(1.05); }
    button:not(:disabled):active { transform: translateY(1px); }
    button:focus-visible, input:focus-visible {
      outline: 2px solid var(--accent);
      outline-offset: 2px;
    }
    input::placeholder { color: var(--muted); opacity: .85; }
    tbody tr { transition: background .12s; }
    tbody tr:hover { background: var(--row-hover); }
    .ell { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  `;

  return (
    <div style={css.page}>
      <style>{globalCss}</style>
      {/* Brand */}
      <header
        style={{
          display: "flex",
          gap: 12,
          alignItems: "center",
          flexWrap: "wrap",
          marginBottom: 14,
        }}
      >
        <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
          <div style={{ display: "flex", alignItems: "baseline", gap: 10 }}>
            <h1 style={{ fontSize: 26, fontWeight: 700, margin: 0, letterSpacing: -0.3 }}>
              {L.title}
            </h1>
            <span style={{ ...css.muted, fontSize: 12 }}>{L.subtitle}</span>
          </div>
          <div style={{ display: "flex", gap: 8, flexWrap: "wrap", alignItems: "center" }}>
            <span style={css.chip}>
              {loading ? "…" : `${filtered.length} / ${apps.length}`}
            </span>
            {admin !== null && (
              <span
                style={{
                  ...css.chip,
                  color: admin ? "var(--accent)" : "var(--danger)",
                  borderColor: admin ? "var(--accent)" : "var(--danger)",
                }}
              >
                {admin ? L.admin : L.nonAdmin}
              </span>
            )}
            {disk && <span style={css.chip}>{`${L.disk} ${disk}`}</span>}
            {estimating && <span style={css.chip}>{L.estimatingSizes}</span>}
            {monitoring && <span style={css.chip}>{L.monitorRunning}</span>}
          </div>
        </div>
        <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
          <button
            style={css.btnSm}
            onClick={() => setTheme((th) => (th === "dark" ? "light" : "dark"))}
          >
            {L.themeToggle}
          </button>
          <button
            style={css.btnSm}
            onClick={() => {
              const next = currentLang() === "zh" ? "en" : "zh";
              setLang(next);
              setLangVer((v) => v + 1);
            }}
          >
            {L.langToggle}
          </button>
        </div>
      </header>

      {showGuide && (
        <div
          style={{
            ...css.card,
            padding: "10px 14px",
            marginBottom: 12,
            background: "var(--surface-2)",
            fontSize: 13,
            display: "flex",
            gap: 12,
            alignItems: "center",
          }}
        >
          <span style={{ flex: 1 }}>{L.guided}</span>
          <button
            style={css.btnSm}
            onClick={() => {
              localStorage.setItem("remova_guided", "1");
              setShowGuide(false);
            }}
          >
            {L.closeGuide}
          </button>
        </div>
      )}
      {notice && (
        <div style={{ marginBottom: 8, fontSize: 13, color: "var(--muted)" }}>{notice}</div>
      )}

      {/* Search + primary actions */}
      <div style={css.toolbar}>
        <input
          style={{ ...css.input, flex: "1 1 280px" }}
          placeholder={L.search}
          value={q}
          onChange={(e) => setQ(e.target.value)}
          aria-label={L.search}
        />
        {batching ? (
          <button style={{ ...css.btn, background: "#b45309", color: "#fff" }} onClick={() => cancelBatch()}>
            {L.batchCancel}
          </button>
        ) : (
          <button
            style={{ ...css.btn, opacity: multi.size === 0 ? 0.5 : 1 }}
            disabled={multi.size === 0}
            title={multi.size === 0 ? L.selectRowHint : undefined}
            onClick={() => void batchCleanup()}
          >
            {L.batch} ({multi.size})
          </button>
        )}
        <button
          style={{
            ...css.btn,
            opacity: !analyzeApp || scanning ? 0.5 : 1,
          }}
          disabled={!analyzeApp || scanning}
          title={!analyzeApp ? L.selectRowHint : undefined}
          onClick={() => analyzeApp && analyze(analyzeApp)}
        >
          {scanning ? L.analyzing : L.analyze}
        </button>
        <button
          style={css.btnGhost}
          aria-expanded={showTools}
          title={showTools ? L.toolsCollapseHint : L.toolsExpandHint}
          onClick={() => setShowTools((v) => !v)}
        >
          {showTools ? "收起工具" : "更多工具"}
        </button>
        <button
          style={css.btnGhost}
          disabled={admin === true || admin === null}
          title={admin === true ? L.adminAlready : L.adminHint}
          onClick={async () => {
            try {
              await invoke("elevate_restart");
            } catch (e) {
              setError(formatError(e, "elevate"));
            }
          }}
        >
          Admin
        </button>
      </div>

      {/* Secondary tools (collapsed by default — reduces visual noise) */}
      {showTools && (
        <div
          style={{
            ...css.toolbar,
            padding: "10px 12px",
            marginBottom: 10,
            background: "var(--surface-2)",
            border: "1px solid var(--border)",
            borderRadius: 12,
          }}
        >
          <button style={css.btnGhost} title={L.historyHint} onClick={async () => {
            const h = await invoke<
              { app_name: string; deleted: number; failed: number; backup_dir: string }[]
            >("list_cleanup_history");
            setHistory(h);
            setShowHistory(true);
          }}>
            {L.history}
          </button>
          <button style={css.btnGhost} title={L.exportCsvHint} onClick={async () => {
            const csv = await invoke<string>("export_history_csv");
            const blob = new Blob([csv], { type: "text/csv;charset=utf-8" });
            const url = URL.createObjectURL(blob);
            const a = document.createElement("a");
            a.href = url;
            a.download = "remova-history.csv";
            a.click();
            URL.revokeObjectURL(url);
          }}>
            {L.exportCsv}
          </button>
          <button style={css.btnGhost} title={L.restoreHint} onClick={() => void openRestoreSessions()}>
            {L.restore}
          </button>
          <button style={css.btnGhost} title={showManage ? L.manageCloseHint : L.manageHint} onClick={() => {
            if (showManage) {
              setShowManage(false);
              return;
            }
            setShowManage(true);
            void loadManage("startup");
          }}>
            {showManage ? L.manageClose : L.manage}
          </button>
          <button
            style={css.btnGhost}
            disabled={!selected || forceBusy || scanning}
            title={selected ? L.forceCleanHint : L.selectRowHint}
            onClick={() => void forceClean()}
          >
            {L.forceClean}
          </button>
          <button style={css.btnGhost} title={L.ignorePublisherHint} disabled={!selected?.publisher} onClick={() => void doIgnorePublisher()}>
            {L.ignorePublisher}
          </button>
          <button style={css.btnGhost} title={L.ignoreAppHint} disabled={!selected?.name} onClick={() => void doIgnoreApp()}>
            {L.ignoreApp}
          </button>
          <button style={css.btnGhost} title={L.orphanScanHint} onClick={() => void runOrphanScan()}>
            {L.orphanScan}
          </button>
          <button style={css.btnGhost} title={monitoring ? L.monitorStopHint : L.monitorInstallHint} onClick={() => void toggleMonitor()}>
            {monitoring ? L.monitorStop : L.monitorInstall}
          </button>
          {lastReport && (
            <button style={css.btnGhost} title={L.exportReportHint} onClick={exportHtmlReport}>
              {L.exportReport}
            </button>
          )}
          <button
            style={css.btnGhost}
            title={L.openReleasesHint}
            onClick={() => {
              window.open("https://github.com/Tsuki-hash/Remova/releases", "_blank");
            }}
          >
            {L.openReleases}
          </button>
          <button
            style={css.btnGhost}
            title={shellMenu ? L.shellUnregisterHint : L.shellMenuHint}
            onClick={async () => {
              try {
                if (shellMenu) {
                  await invoke("unregister_context_menu");
                  setShellMenu(false);
                  setNotice(L.shellUnregister);
                } else {
                  await invoke("register_context_menu");
                  setShellMenu(true);
                  setNotice(L.shellMenuOn);
                }
              } catch (e) {
                setError(formatError(e));
              }
            }}
          >
            {shellMenu ? L.shellUnregister : L.shellMenu}
          </button>
          {estimating && (
            <button style={css.btnGhost} title={L.stopEstimateHint} onClick={() => void stopSizeEstimate()}>
              {L.stopEstimate}
            </button>
          )}
        </div>
      )}

      {scan && (
        <div
          style={{
            ...css.toolbar,
            marginBottom: 10,
            padding: "10px 12px",
            background: "var(--surface-2)",
            border: "1px solid var(--border)",
            borderRadius: 12,
          }}
        >
          <button
            style={css.btnGhost}
            onClick={() => {
              setScan(null);
              setReport(null);
              setError(null);
            }}
          >
            ← {L.closePreview}
          </button>
          <strong style={{ fontSize: 13 }}>{scan.app_name}</strong>
          <label style={{ fontSize: 13, display: "flex", alignItems: "center", gap: 6 }}>
            <input
              type="checkbox"
              checked={useOfficial}
              onChange={(e) => setUseOfficial(e.target.checked)}
            />
            {L.useOfficial}
          </label>
          <button style={css.btnGhost} disabled={dryRunning || selectedPaths.size === 0} onClick={dryRun}>
            {L.dryRun}
          </button>
          <button
            style={{ ...css.btn, background: "var(--danger)", color: "#fff" }}
            disabled={dryRunning || selectedPaths.size === 0 || busyRef.current}
            onClick={() => {
              if (!window.confirm(L.cleanupConfirm(selectedPaths.size, useOfficial))) return;
              void execReal();
            }}
          >
            {L.cleanup} ({selectedPaths.size})
          </button>
        </div>
      )}

      {report && (
        <div
          style={{
            ...css.card,
            marginBottom: 10,
            padding: 12,
            fontSize: 13,
            background: "var(--surface-2)",
          }}
        >
          <div style={{ display: "flex", gap: 12, alignItems: "center", flexWrap: "wrap" }}>
            <strong>
              {"dry_run" in report && report.dry_run ? L.dryRunSummary : L.batchSummary}: {report.app_name}
            </strong>
            {"deleted_planned" in report ? (
              <span>
                {L.dryRunPlanned}: {report.deleted_planned} · {L.batchSkipped}: {report.skipped}
              </span>
            ) : (
              <span>
                {L.batchOk}: {report.deleted} · {L.batchFailed}: {report.failed} · {L.batchSkipped}:{" "}
                {report.skipped}
              </span>
            )}
            <button style={{ ...css.btnGhost, height: 28 }} onClick={() => setReport(null)}>
              ×
            </button>
          </div>
          {report.item_details.length > 0 && (
            <div style={{ maxHeight: 160, overflow: "auto", marginTop: 8, color: "var(--muted)" }}>
              {report.item_details.slice(0, 50).map((d, i) => (
                <div key={i} className="ell" title={d.path}>
                  [{d.status}] {d.path}
                  {d.message ? ` — ${d.message}` : ""}
                </div>
              ))}
              {report.item_details.length > 50 && (
                <div>… +{report.item_details.length - 50}</div>
              )}
            </div>
          )}
          {"uninstall_message" in report && report.uninstall_message && (
            <div style={{ marginTop: 6, color: "var(--muted)" }}>{report.uninstall_message}</div>
          )}
        </div>
      )}

      {showHistory && (
        <div style={{ ...css.card, marginBottom: 12, padding: 12, fontSize: 13 }}>
          <strong>{L.historyTitle}</strong>
          <input
            style={{ ...css.input, maxWidth: 220, height: 32, marginLeft: 12 }}
            placeholder={L.search}
            value={histQ}
            onChange={(e) => setHistQ(e.target.value)}
          />
          <button style={{ marginLeft: 12, ...css.btnGhost }} onClick={() => setShowHistory(false)}>
            ×
          </button>
          <div style={{ maxHeight: 160, overflow: "auto", marginTop: 8 }}>
            {history.filter(
              (h) => !histQ.trim() || h.app_name.toLowerCase().includes(histQ.trim().toLowerCase()),
            ).length === 0 && <div>{L.noHistory}</div>}
            {history
              .filter(
                (h) =>
                  !histQ.trim() || h.app_name.toLowerCase().includes(histQ.trim().toLowerCase()),
              )
              .map((h, i) => (
                <div key={i}>
                  {h.app_name} · {h.deleted}/{h.failed}
                  {h.backup_dir ? ` · ${h.backup_dir}` : ""}
                </div>
              ))}
          </div>
        </div>
      )}

      {showRestore && (
        <div style={{ ...css.card, marginBottom: 12, padding: 12, fontSize: 13 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 8 }}>
            <strong>{L.restoreSessions}</strong>
            <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
              <button
                style={{ ...css.btn, height: 32, opacity: restoreBusy || !restorePick ? 0.5 : 1 }}
                disabled={restoreBusy || !restorePick}
                onClick={() => void runRestoreSession()}
              >
                {L.restoreRun}
              </button>
              <button
                style={{ ...css.btnGhost, height: 32 }}
                onClick={() => setShowRestore(false)}
              >
                {L.restoreClose}
              </button>
            </div>
          </div>
          {restoreSessions.length === 0 ? (
            <div style={css.muted}>{L.restoreNoSessions}</div>
          ) : (
            <>
              <div style={{ ...css.muted, marginBottom: 6 }}>{L.restoreSelect}</div>
              <div style={{ maxHeight: 180, overflow: "auto" }}>
                {restoreSessions.map((name) => (
                  <label
                    key={name}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: 8,
                      padding: "6px 4px",
                      borderBottom: "1px solid var(--border)",
                      cursor: "pointer",
                    }}
                  >
                    <input
                      type="radio"
                      name="restore-session"
                      checked={restorePick === name}
                      onChange={() => setRestorePick(name)}
                    />
                    <span>{name}</span>
                  </label>
                ))}
              </div>
            </>
          )}
          {restoreMsgs.length > 0 && (
            <div style={{ marginTop: 10 }}>
              <strong>{L.restoreResult}</strong>
              <pre
                style={{
                  margin: "6px 0 0",
                  whiteSpace: "pre-wrap",
                  fontSize: 12,
                  color: "var(--muted)",
                }}
              >
                {restoreMsgs.slice(0, 20).join("\n")}
              </pre>
            </div>
          )}
        </div>
      )}

      {monitorDiff && (
        <div style={{ ...css.card, marginBottom: 12, padding: 12, fontSize: 13 }}>
          <strong>{L.monitorDiff}</strong>
          <div style={{ ...css.muted, margin: "6px 0" }}>
            files={monitorDiff.added_files.length} · reg={monitorDiff.added_reg_values.length}
          </div>
          <div style={{ maxHeight: 160, overflow: "auto" }}>
            {[...monitorDiff.added_files, ...monitorDiff.added_reg_values]
              .slice(0, 80)
              .map((line) => (
                <div key={line} style={{ wordBreak: "break-all", padding: "2px 0" }}>
                  {line}
                </div>
              ))}
          </div>
          <button
            style={{ ...css.btnGhost, height: 30, marginTop: 8 }}
            onClick={() => setMonitorDiff(null)}
          >
            {L.batchDismiss}
          </button>
        </div>
      )}

      {showManage && (
        <div style={{ ...css.card, marginBottom: 12, padding: 12, fontSize: 13 }}>
          <div style={{ display: "flex", gap: 8, marginBottom: 8, alignItems: "center" }}>
            <strong>{L.manage}</strong>
            {(["startup", "services", "tasks"] as ManageTab[]).map((tab) => (
              <button
                key={tab}
                style={{
                  ...css.btnGhost,
                  height: 30,
                  borderColor: manageTab === tab ? "var(--accent)" : undefined,
                }}
                onClick={() => void loadManage(tab)}
              >
                {tab === "startup"
                  ? L.manageStartup
                  : tab === "services"
                    ? L.manageServices
                    : L.manageTasks}
              </button>
            ))}
            <button
              style={{ ...css.btnGhost, height: 30 }}
              onClick={() => void loadManage(manageTab)}
            >
              {L.manageReload}
            </button>
            <button
              style={{ ...css.btnGhost, height: 30, marginLeft: "auto" }}
              onClick={() => setShowManage(false)}
            >
              ×
            </button>
          </div>
          <div style={{ maxHeight: 240, overflow: "auto" }}>
            {manageBusy && <div style={css.muted}>{L.estimatingSizes}</div>}
            {!manageBusy && manageItems.length === 0 && (
              <div style={css.muted}>{L.noHistory}</div>
            )}
            {!manageBusy &&
              manageItems.slice(0, 200).map((it) => (
                <div
                  key={it.location + it.name}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 10,
                    padding: "6px 2px",
                    borderBottom: "1px solid var(--border)",
                  }}
                >
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div>{it.name}</div>
                    <div style={{ ...css.muted, fontSize: 12, wordBreak: "break-all" }}>
                      {it.detail || it.location}
                    </div>
                  </div>
                  <span style={{ color: it.enabled ? "var(--accent)" : "var(--muted)" }}>
                    {it.enabled ? "●" : "○"}
                  </span>
                  <button
                    style={{ ...css.btnGhost, height: 28 }}
                    disabled={manageBusy}
                    onClick={() => void toggleManageItem(it)}
                  >
                    {it.enabled ? L.manageDisable : L.manageEnable}
                  </button>
                </div>
              ))}
          </div>
        </div>
      )}

      {batching && batchTotal > 0 && (
        <div
          style={{
            ...css.card,
            marginBottom: 12,
            padding: "10px 14px",
            fontSize: 13,
          }}
        >
          <div style={{ display: "flex", justifyContent: "space-between", gap: 12 }}>
            <span>
              {batchIndex}/{batchTotal} · {batchCurrent}
            </span>
            <span style={css.muted}>{L.batchCancelHint}</span>
          </div>
          <div
            style={{
              marginTop: 8,
              height: 6,
              borderRadius: 3,
              background: "var(--th-bg)",
              overflow: "hidden",
            }}
          >
            <div
              style={{
                width: `${batchTotal ? (batchIndex / batchTotal) * 100 : 0}%`,
                height: "100%",
                background: "var(--accent)",
                transition: "width 0.2s",
              }}
            />
          </div>
        </div>
      )}

      {showBatchSummary && batchResults.length > 0 && (
        <div style={{ ...css.card, marginBottom: 12, padding: 12, fontSize: 13 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 8 }}>
            <strong>{L.batchSummary}</strong>
            <span style={css.muted}>
              {L.batchOk} {batchResults.filter((r) => r.status === "ok").length} ·{" "}
              {L.batchFailed} {batchResults.filter((r) => r.status === "failed").length} ·{" "}
              {L.batchSkipped} {batchResults.filter((r) => r.status === "skipped").length}
            </span>
            <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
              {batchResults.some((r) => r.status === "failed") && (
                <button style={{ ...css.btnGhost, height: 32 }} onClick={retryFailedBatch}>
                  {L.batchRetryFailed}
                </button>
              )}
              <button
                style={{ ...css.btnGhost, height: 32 }}
                onClick={() => setShowBatchSummary(false)}
              >
                {L.batchDismiss}
              </button>
            </div>
          </div>
          <div style={{ maxHeight: 180, overflow: "auto" }}>
            {batchResults.map((r) => (
              <div key={r.key} style={{ padding: "4px 0", borderBottom: "1px solid var(--border)" }}>
                <span
                  style={{
                    color:
                      r.status === "failed"
                        ? "#b91c1c"
                        : r.status === "ok"
                          ? "var(--accent)"
                          : "var(--muted)",
                    marginRight: 8,
                  }}
                >
                  {r.status === "ok"
                    ? L.batchOk
                    : r.status === "failed"
                      ? L.batchFailed
                      : L.batchSkipped}
                </span>
                {r.name}
                {r.detail ? <span style={css.muted}> · {r.detail}</span> : null}
              </div>
            ))}
          </div>
        </div>
      )}

      {error && (
        <div
          style={{
            display: "flex",
            alignItems: "flex-start",
            gap: 12,
            marginBottom: 12,
            padding: "10px 12px",
            borderRadius: 10,
            border: "1px solid #fca5a5",
            background: "var(--surface)",
            color: "#b91c1c",
            fontSize: 13,
            lineHeight: 1.45,
          }}
          role="alert"
        >
          <span style={{ flex: 1 }}>{error}</span>
          <button
            style={{ ...css.btnGhost, height: 28, padding: "0 10px", color: "#b91c1c", flexShrink: 0 }}
            onClick={() => setError(null)}
          >
            {L.errorDismiss}
          </button>
        </div>
      )}

      {scan ? (
        <div style={css.card}>
          <div style={{ padding: "12px 16px", borderBottom: "1px solid var(--border)" }}>
            <strong>{scan.app_name}</strong>
            <span style={{ ...css.muted, marginLeft: 12 }}>
              {scan.items.length} items ·{" "}
              {scan.items.filter((i) => i.confidence === "confirmed").length} confirmed
            </span>
          </div>
          <div style={css.scroll}>
            <table style={css.table}>
              <thead>
                <tr>
                  <th style={css.th}>✓</th>
                  <th style={css.th}>{L.colLocation}</th>
                  <th style={css.th}>type</th>
                  <th style={css.th}>score</th>
                  <th style={css.th}>match</th>
                  <th style={css.th}>risk</th>
                  <th style={css.th}>evidence</th>
                </tr>
              </thead>
              <tbody>
                {scan.items.map((it) => (
                  <tr key={it.path}>
                    <td style={css.td}>
                      <input
                        type="checkbox"
                        checked={selectedPaths.has(it.path)}
                        onChange={() =>
                          setSelectedPaths((s) => {
                            const n = new Set(s);
                            if (n.has(it.path)) n.delete(it.path);
                            else n.add(it.path);
                            return n;
                          })
                        }
                      />
                    </td>
                    <td style={css.td}>{it.path}</td>
                    <td style={css.td}>{it.kind}</td>
                    <td style={css.td}>{it.score}</td>
                    <td style={css.td}>
                      {it.confidence === "confirmed"
                        ? L.confirmed
                        : it.score >= 30
                          ? L.suspected
                          : L.low}
                    </td>
                    <td style={css.td}>{it.risk}</td>
                    <td style={css.td}>
                      <button
                        style={{ ...css.btnGhost, height: 28 }}
                        onClick={() =>
                          setEvidence(
                            it.evidence
                              .map(
                                (e) =>
                                  `${e.label} (${e.weight})${e.detail ? " — " + e.detail : ""}`,
                              )
                              .join("\n") || it.reason,
                          )
                        }
                      >
                        ⓘ
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {evidence && (
            <div
              style={{
                padding: 12,
                borderTop: "1px solid var(--border)",
                fontSize: 13,
                whiteSpace: "pre-wrap",
                background: "var(--th-bg)",
              }}
            >
              {evidence}{" "}
              <button style={{ ...css.btnGhost, height: 28 }} onClick={() => setEvidence(null)}>
                ×
              </button>
            </div>
          )}
        </div>
      ) : (
        <div style={css.card}>
          <div style={css.scroll}>
            <table style={css.table}>
              <thead>
                <tr>
                  <th style={{ ...css.th, width: 48, whiteSpace: "nowrap" }}>✓</th>
                  <th
                    style={{ ...css.th, cursor: "pointer", whiteSpace: "nowrap" }}
                    onClick={() => sortBy("name")}
                  >
                    {L.colName} {sortCol === "name" ? (sortDesc ? "↓" : "↑") : ""}
                  </th>
                  <th style={{ ...css.th, whiteSpace: "nowrap" }}>{L.colVersion}</th>
                  <th
                    style={{ ...css.th, cursor: "pointer", whiteSpace: "nowrap" }}
                    onClick={() => sortBy("publisher")}
                  >
                    {L.colPublisher} {sortCol === "publisher" ? (sortDesc ? "↓" : "↑") : ""}
                  </th>
                  <th
                    style={{ ...css.th, cursor: "pointer", width: 90, minWidth: 90, whiteSpace: "nowrap" }}
                    onClick={() => sortBy("size")}
                  >
                    {L.colSize} {sortCol === "size" ? (sortDesc ? "↓" : "↑") : ""}
                  </th>
                  <th
                    style={{
                      ...css.th,
                      cursor: "pointer",
                      whiteSpace: "nowrap",
                      minWidth: 110,
                    }}
                    onClick={() => sortBy("install_date")}
                  >
                    {L.colInstallDate}{" "}
                    {sortCol === "install_date" ? (sortDesc ? "↓" : "↑") : ""}
                  </th>
                  <th style={{ ...css.th, whiteSpace: "nowrap" }}>{L.colSource}</th>
                  <th
                    style={{ ...css.th, cursor: "pointer" }}
                    onClick={() => sortBy("install_location")}
                  >
                    {L.colLocation}{" "}
                    {sortCol === "install_location" ? (sortDesc ? "↓" : "↑") : ""}
                  </th>
                </tr>
              </thead>
              <tbody>
                {filtered.map((a) => {
                  const key = a.registry_key + a.name;
                  return (
                    <tr
                      key={key}
                      onClick={() => setSelected(a)}
                      onDoubleClick={() => analyze(a)}
                      title={[
                        prettyAppName(a.name, a.source),
                        a.publisher && `${L.colPublisher}: ${a.publisher}`,
                        a.install_location && `${L.colLocation}: ${a.install_location}`,
                        (a.quiet_uninstall_string || a.uninstall_string) &&
                          `Uninstall: ${a.quiet_uninstall_string || a.uninstall_string}`,
                        a.registry_key && `Key: ${a.registry_key}`,
                      ]
                        .filter(Boolean)
                        .join("\n") || undefined}
                      style={{
                        cursor: "pointer",
                        background:
                          selected?.registry_key === a.registry_key && selected.name === a.name
                            ? "var(--th-bg)"
                            : undefined,
                      }}
                    >
                      <td style={css.td}>
                        <input
                          type="checkbox"
                          checked={multi.has(key)}
                          onChange={() => {
                            const next = !multi.has(key);
                            toggleMulti(key);
                            if (next && !selected) setSelected(a);
                          }}
                          onClick={(e) => e.stopPropagation()}
                        />
                      </td>
                      <td style={css.td}>
                        <div style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
                          <AppIcon displayIcon={a.display_icon} name={a.name} />
                          <span className="ell" style={{ maxWidth: 280 }} title={a.name}>
                            {prettyAppName(a.name, a.source)}
                          </span>
                        </div>
                      </td>
                      <td style={{ ...css.td, whiteSpace: "nowrap", color: "var(--muted)" }}>
                        {a.version || "—"}
                      </td>
                      <td style={css.td}>
                        <span className="ell" style={{ display: "block", maxWidth: 180 }} title={a.publisher}>
                          {prettyPublisher(a.publisher)}
                        </span>
                      </td>
                      <td style={{ ...css.td, textAlign: "right" as const, whiteSpace: "nowrap", fontVariantNumeric: "tabular-nums" }}>
                        {formatAppSize(a)}
                      </td>
                      <td style={{ ...css.td, whiteSpace: "nowrap", color: "var(--muted)" }}>
                        {a.install_date || "—"}
                      </td>
                      <td style={css.td}>
                        <span
                          style={{
                            display: "inline-block",
                            padding: "2px 8px",
                            borderRadius: 6,
                            fontSize: 11,
                            background: a.source === "Store" ? "var(--surface-2)" : "transparent",
                            border: "1px solid var(--border)",
                          }}
                        >
                          {a.source}
                        </span>
                      </td>
                      <td style={css.td}>
                        <span className="ell" style={{ display: "block", maxWidth: 320, color: "var(--muted)", fontSize: 12 }} title={a.install_location}>
                          {a.install_location ? shortPath(a.install_location) : "—"}
                        </span>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  );
}
