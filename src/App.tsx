import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useVirtualizer } from "@tanstack/react-virtual";
import type {
  CleanupReport,
  FullCleanupReport,
  InstalledApp,
  OfficialUninstallResult,
  ScanResult,
} from "./types";
import { currentLang, formatSize, loadLang, setLang, t } from "./i18n";
import { compareSemver } from "./semver";
import { cssStyles as css, globalCss } from "./styles";
import { AppRow } from "./components/AppRow";
import {
  BatchProgress,
  BatchSummaryPanel,
  type BatchItemResult,
} from "./components/BatchPanels";
import { escapeHtml, formatError, prettyAppName } from "./lib/format";
import { applyTheme, loadNav, loadTheme, saveNav, type NavId, type Theme } from "./lib/theme";
import { Shell } from "./components/Shell";
import { ManageListPage } from "./components/ManageListPage";
import { MorePage } from "./components/MorePage";

declare const __APP_VERSION__: string;

/** Stable row/multi-select key (CODE-8: avoid ambiguous string concat). */
function appKey(a: InstalledApp): string {
  return `${a.source}\u0000${a.registry_key}\u0000${a.name}`;
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
  /** Beginner batch path runs official uninstaller by default. */
  const batchUseOfficial = true;
  /** Deep-analyze leftover path can still opt into official uninstaller. */
  const [useOfficial, setUseOfficial] = useState(false);
  const [sourceFilter, setSourceFilter] = useState<string>("");
  const [sortCol, setSortCol] = useState<"name" | "size" | null>(null);
  const [sortDesc, setSortDesc] = useState(false);
  const [admin, setAdmin] = useState<boolean | null>(null);
  const [disk, setDisk] = useState("");
  const [theme, setTheme] = useState<Theme>(loadTheme());
  const [nav, setNav] = useState<NavId>(loadNav());
  const [category, setCategoryState] = useState<"all" | "desktop" | "store" | "large" | "recent">(
    () => {
      const v = localStorage.getItem("remova_cat");
      return v === "desktop" || v === "store" || v === "large" || v === "recent" ? v : "all";
    },
  );
  const setCategory = useCallback((id: "all" | "desktop" | "store" | "large" | "recent") => {
    setCategoryState(id);
    localStorage.setItem("remova_cat", id);
  }, []);
  const [langVer, setLangVer] = useState(0);
  const [showGuide, setShowGuide] = useState(!localStorage.getItem("remova_guided"));
  const [notice, setNotice] = useState<string | null>(null);
  const [batching, setBatching] = useState(false);
  const [batchIndex, setBatchIndex] = useState(0);
  const [batchTotal, setBatchTotal] = useState(0);
  const [batchCurrent, setBatchCurrent] = useState("");
  const [batchResults, setBatchResults] = useState<BatchItemResult[]>([]);
  const [showBatchSummary, setShowBatchSummary] = useState(false);
  const batchCancelRef = useRef(false);
  const [forceBusy, setForceBusy] = useState(false);
  const [shellMenu, setShellMenu] = useState(false);
  const [uninstallingKey, setUninstallingKey] = useState<string | null>(null);
  const [officialResult, setOfficialResult] = useState<OfficialUninstallResult | null>(null);
  const [postUninstallApp, setPostUninstallApp] = useState<InstalledApp | null>(null);
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

  const goNav = useCallback((n: NavId) => {
    setNav(n);
    saveNav(n);
  }, []);

  const analyze = useCallback(async (app: InstalledApp) => {
    goNav("software");
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
  }, [goNav]);

  const refreshApps = useCallback(async () => {
    try {
      const list = await invoke<InstalledApp[]>("list_installed_apps");
      setApps(list);
    } catch (e) {
      setError(formatError(e));
    }
  }, []);

  const startUninstall = useCallback(
    async (app: InstalledApp) => {
      const strings = t();
      const label = prettyAppName(app.name, app.source);
      if (!window.confirm(strings.uninstallConfirm(label))) return;
      const key = appKey(app);
      setUninstallingKey(key);
      setSelected(app);
      setOfficialResult(null);
      setPostUninstallApp(null);
      busyRef.current = true;
      try {
        const r = await invoke<OfficialUninstallResult>("run_official_uninstall", { app });
        setOfficialResult(r);
        if (!r.had_command) {
          setNotice(strings.uninstallNoCmd);
        } else if (r.ok) {
          setNotice(strings.uninstallOk);
        } else {
          setNotice(`${strings.uninstallFail}: ${r.message}`);
        }
        if (r.had_command && r.ok) {
          setPostUninstallApp(app);
        }
        await refreshApps();
      } catch (e) {
        setError(formatError(e, "cleanup"));
      } finally {
        busyRef.current = false;
        setUninstallingKey(null);
      }
    },
    [refreshApps],
  );

  const scanLeftoversFor = useCallback(
    async (app: InstalledApp) => {
      setPostUninstallApp(null);
      setOfficialResult(null);
      goNav("software");
      await analyze(app);
    },
    [analyze, goNav],
  );

  const skipLeftovers = useCallback(async () => {
    setPostUninstallApp(null);
    setOfficialResult(null);
    setScan(null);
    setReport(null);
    await refreshApps();
  }, [refreshApps]);

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
    void invoke<{ free_gb: number; total_gb: number; drive?: string }>("disk_usage")
      .then((d) => {
        const drive = (d.drive || localStorage.getItem("remova_disk_drive") || "C:")
          .slice(0, 2)
          .toUpperCase();
        localStorage.setItem("remova_disk_drive", drive);
        setDisk(`${drive} ${d.free_gb.toFixed(1)} / ${d.total_gb.toFixed(0)} GB`);
      })
      .catch(() => {});
    // Update check: compare semver, 8s timeout (FUNC-7)
    void (async () => {
      try {
        const ctrl = new AbortController();
        const timer = setTimeout(() => ctrl.abort(), 8000);
        const r = await fetch(
          "https://api.github.com/repos/Tsuki-hash/Remova/releases/latest",
          {
            headers: { Accept: "application/vnd.github+json" },
            signal: ctrl.signal,
          },
        );
        clearTimeout(timer);
        if (!r.ok) return;
        const j = (await r.json()) as { tag_name?: string } | null;
        const tag = j?.tag_name?.replace(/^v/i, "");
        if (!tag) return;
        const cur = __APP_VERSION__;
        if (compareSemver(tag, cur) > 0) {
          setNotice(`${t().versionNew}: v${tag}`);
        }
      } catch {
        // offline / blocked — ignore
      }
    })();
    // Context menu --analyze handoff runs after apps load (see effect below)
    // tauri window close confirm
    void (async () => {
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        const win = getCurrentWindow();
        await win.onCloseRequested(async (event) => {
          if (busyRef.current) {
            if (
              !window.confirm(
                t().closeConfirmBusy,
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

  // Context menu --analyze: match path to an app after list load (FUNC-2).
  useEffect(() => {
    if (loading || apps.length === 0) return;
    let cancelled = false;
    void invoke<string | null>("take_pending_analyze")
      .then((p) => {
        if (cancelled || !p) return;
        const low = p.toLowerCase();
        const hit = apps.find(
          (a) =>
            a.install_location &&
            low.startsWith(a.install_location.replace(/\//g, "\\").toLowerCase()),
        );
        if (hit) {
          goNav("software");
          setSelected(hit);
          void analyze(hit);
        } else {
          setQ(p);
          setNotice(p);
        }
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loading, apps]);

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
    if (category === "desktop") {
      list = list.filter((a) => a.source !== "Store");
    } else if (category === "store") {
      list = list.filter((a) => a.source === "Store");
    } else if (category === "large") {
      list = [...list]
        .filter((a) => sizeOf(a) > 200 * 1024)
        .sort((a, b) => sizeOf(b) - sizeOf(a));
    } else if (category === "recent") {
      list = [...list]
        .filter((a) => a.install_date)
        .sort((a, b) => (b.install_date || "").localeCompare(a.install_date || ""))
        .slice(0, 50);
    }
    if (sourceFilter) {
      list = list.filter((a) => a.source === sourceFilter);
    }
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
      return (a.name || "").toLowerCase().localeCompare((b.name || "").toLowerCase());
    });
    return sortDesc ? s.reverse() : s;
  }, [apps, q, sortCol, sortDesc, sizeOf, sizeMap, ignorePub, ignoreName, sourceFilter, category]);

  const sortBy = (col: "name" | "size") => {
    if (sortCol === col) setSortDesc((d) => !d);
    else {
      setSortCol(col);
      setSortDesc(col === "size");
    }
  };

  // Virtualize the app list (PERF-2): only render visible rows.
  const listScrollRef = useRef<HTMLDivElement | null>(null);
  const rowVirtualizer = useVirtualizer({
    count: filtered.length,
    getScrollElement: () => listScrollRef.current,
    estimateSize: () => 56,
    overscan: 12,
  });
  const virtualRows = rowVirtualizer.getVirtualItems();
  const totalSize = rowVirtualizer.getTotalSize();

  /** App used by 深度分析: row click, else the single multi-checkbox target. */
  const analyzeApp = useMemo(() => {
    if (selected) return selected;
    if (multi.size === 1) {
      const key = [...multi][0];
      return apps.find((a) => appKey(a) === key) ?? null;
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

  const forceClean = useCallback(
    async (appOverride?: InstalledApp) => {
      const target = appOverride ?? selected;
      if (!target || forceBusy) return;
      if (!window.confirm(L.forceCleanHint)) return;
      setForceBusy(true);
      busyRef.current = true;
      try {
        const r = await invoke<ScanResult>("analyze_associations", { app: target });
        const items = r.items.filter(
          (it) => it.confidence === "confirmed" && it.risk !== "high",
        );
        if (!items.length) {
          setNotice(L.noHistory);
          return;
        }
        const report = await invoke<FullCleanupReport>("run_full_cleanup", {
          app: target,
          items,
          options: {
            dry_run: false,
            skip_official_uninstall: true,
            backup_enabled: true,
          },
        });
        setNotice(`${L.forceClean}: ${target.name} · ${L.batchDetail(report.deleted, report.failed)}`);
        setLastReport(report);
        void refreshApps();
      } catch (e) {
        setError(formatError(e, "cleanup"));
      } finally {
        busyRef.current = false;
        setForceBusy(false);
      }
    },
    [selected, forceBusy, L, refreshApps],
  );

  const doIgnorePublisher = useCallback(
    async (appOverride?: InstalledApp) => {
      const pub = (appOverride ?? selected)?.publisher;
      if (!pub) return;
      try {
        const ig = await invoke<{ publishers: string[]; names: string[] }>("ignore_publisher", {
          name: pub,
        });
        setIgnorePub(ig.publishers || []);
        setNotice(L.ignoreLoaded);
      } catch (e) {
        setError(formatError(e));
      }
    },
    [selected, L],
  );

  const doIgnoreApp = useCallback(
    async (appOverride?: InstalledApp) => {
      const name = (appOverride ?? selected)?.name;
      if (!name) return;
      try {
        const ig = await invoke<{ publishers: string[]; names: string[] }>("ignore_app_name", {
          name,
        });
        setIgnoreName(ig.names || []);
        setNotice(L.ignoreLoaded);
      } catch (e) {
        setError(formatError(e));
      }
    },
    [selected, L],
  );

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
<p>${r.dry_run ? L.dryRunSummary : L.batchSummary} · ${L.batchOk}: ${r.deleted} · ${L.batchFailed}: ${r.failed} · ${L.batchSkipped}: ${r.skipped}${r.aborted ? " · aborted" : ""}</p>
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
      goNav("software");
      setScan({
        app_name: L.orphanScan,
        items,
      });
      setSelectedPaths(new Set(items.filter((i) => i.confidence === "confirmed").map((i) => i.path)));
      setNotice(items.length === 0 ? L.orphanScanEmpty : `${L.orphanScan}: ${items.length}`);
    } catch (e) {
      setError(formatError(e, "analyze"));
    }
  }, [L, goNav]);

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

  const monitorDiffToCleanup = useCallback(
    async (diff: { added_files: string[]; added_reg_values: string[] }) => {
      try {
        const items = await invoke<ScanResult["items"]>("monitor_diff_to_items", { diff });
        if (!items.length) {
          setNotice(L.monitorNoSnap);
          return;
        }
        goNav("software");
        setScan({ app_name: L.monitorDiff, items });
        setSelectedPaths(
          new Set(items.filter((i) => i.confidence === "confirmed").map((i) => i.path)),
        );
        setMonitorDiff(null);
        setNotice(`${L.monitorToCleanup}: ${items.length}`);
      } catch (e) {
        setError(formatError(e));
      }
    },
    [L, goNav],
  );

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
          // Post-uninstall residual cleanup never re-runs official uninstaller.
          // Deep-analyze path may still opt in via the checkbox.
          skip_official_uninstall: postUninstallApp ? true : !useOfficial,
          backup_enabled: true,
        },
      });
      setReport(r);
      if (r && typeof r === "object" && "deleted" in r) {
        setLastReport(r as FullCleanupReport);
      }
      setPostUninstallApp(null);
      void refreshApps();
    } catch (e) {
      setError(formatError(e, "cleanup"));
    } finally {
      busyRef.current = false;
      setDryRunning(false);
    }
  }, [scan, selected, selectedPaths, refreshApps, postUninstallApp, useOfficial]);

  const batchCleanup = useCallback(async () => {
    const keys = new Set(multi);
    const queue = apps.filter((a) => keys.has(appKey(a)));
    if (!queue.length) {
      setNotice(L.selectRowHint);
      return;
    }
    if (!window.confirm(L.batchConfirm(queue.length, batchUseOfficial))) return;

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
        const key = appKey(app);
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
              skip_official_uninstall: !batchUseOfficial,
              backup_enabled: true,
            },
          });
          const detail = L.batchDetail(report.deleted, report.failed);
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

  return (
    <>
      <style>{globalCss}</style>
      <Shell
        nav={nav}
        onNav={goNav}
        title={nav === "software" ? L.navSoftware : nav === "startup" ? L.navStartup : nav === "services" ? L.navServices : nav === "tasks" ? L.navTasks : L.toolboxTitle}
        subtitle={nav === "software" ? L.subtitle : undefined}
        status={
          nav === "software" ? (
            <>
              <span style={css.chip}>{loading ? "…" : `${filtered.length} / ${apps.length}`}</span>
              {admin !== null && (
                <span style={admin ? css.chipAccent : css.chipDanger}>
                  {admin ? L.admin : L.nonAdmin}
                </span>
              )}
              {disk && <span style={css.chip}>{`${L.disk} ${disk}`}</span>}
              {estimating && <span style={css.chipAccent}>{L.estimatingSizes}</span>}
              {monitoring && <span style={css.chipAccent}>{L.monitorRunning}</span>}
            </>
          ) : null
        }
        actions={
          <>
            <button
              style={css.btnSm}
              title={L.adminHint}
              disabled={admin === true || admin === null}
              onClick={async () => {
                try {
                  await invoke("elevate_restart");
                } catch (e) {
                  setError(formatError(e, "elevate"));
                }
              }}
            >
              {L.adminMenu}
            </button>
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
          </>
        }
      >
        {notice && (
          <div style={{ marginBottom: 8, fontSize: 13, color: "var(--muted)", flexShrink: 0 }}>{notice}</div>
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
              flexShrink: 0,
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
        {nav === "startup" && (
          <ManageListPage tab="startup" title={L.navStartup} onNotice={setNotice} onError={setError} />
        )}
        {nav === "services" && (
          <ManageListPage tab="services" title={L.navServices} onNotice={setNotice} onError={setError} />
        )}
        {nav === "tasks" && (
          <ManageListPage tab="tasks" title={L.navTasks} onNotice={setNotice} onError={setError} />
        )}
        {nav === "more" && (
          <MorePage
            selected={selected}
            monitoring={monitoring}
            monitorDiff={monitorDiff}
            lastReport={lastReport}
            shellMenu={shellMenu}
            onForceClean={() => void forceClean()}
            onIgnorePublisher={() => void doIgnorePublisher()}
            onIgnoreApp={() => void doIgnoreApp()}
            onOrphanScan={() => void runOrphanScan()}
            onToggleMonitor={() => void toggleMonitor()}
            onMonitorToCleanup={() => monitorDiff && void monitorDiffToCleanup(monitorDiff)}
            onDismissMonitor={() => setMonitorDiff(null)}
            onShellToggle={async () => {
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
            onExportReport={exportHtmlReport}
            onError={setError}
            onNotice={setNotice}
          />
        )}
        {nav === "software" && (
          <>
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
                  flexShrink: 0,
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
            <div style={{ ...css.toolbar, flexShrink: 0 }}>
              <input
                style={{ ...css.input, flex: "1 1 220px" }}
                placeholder={L.search}
                value={q}
                onChange={(e) => setQ(e.target.value)}
                aria-label={L.search}
              />
              <select
                style={{ ...css.input, flex: "0 0 auto", minWidth: 110, height: 36 }}
                value={sourceFilter}
                onChange={(e) => setSourceFilter(e.target.value)}
                aria-label={L.colSource}
              >
                <option value="">{L.allSources}</option>
                {Array.from(new Set(apps.map((a) => a.source))).map((s) => (
                  <option key={s} value={s}>
                    {s}
                  </option>
                ))}
              </select>
              {estimating && (
                <button style={css.btnSm} title={L.stopEstimateHint} onClick={() => void stopSizeEstimate()}>
                  {L.stopEstimate}
                </button>
              )}
              {analyzeApp && !scan && (
                <button
                  style={css.btnSm}
                  disabled={scanning}
                  title={L.selectRowHint}
                  onClick={() => analyzeApp && void analyze(analyzeApp)}
                >
                  {scanning ? L.analyzing : L.analyze}
                </button>
              )}
            </div>
            <div style={{ display: "flex", gap: 8, marginBottom: 10, flexShrink: 0, flexWrap: "wrap" }}>
              {(
                [
                  ["all", L.catAll],
                  ["desktop", L.catDesktop],
                  ["store", L.catStore],
                  ["large", L.catLarge],
                  ["recent", L.catRecent],
                ] as const
              ).map(([id, label]) => (
                <button
                  key={id}
                  style={{
                    ...css.btnSm,
                    height: 30,
                    borderRadius: 16,
                    borderColor: category === id ? "var(--accent)" : "var(--border)",
                    color: category === id ? "var(--accent)" : "var(--muted)",
                    background: category === id ? "var(--accent-soft)" : "var(--surface)",
                    fontWeight: category === id ? 650 : 500,
                  }}
                  onClick={() => setCategory(id)}
                >
                  {label}
                </button>
              ))}
            </div>

      {selected && !scan && (
        <div
          style={{
            ...css.card,
            marginBottom: 10,
            padding: "10px 14px",
            fontSize: 12.5,
            display: "flex",
            flexWrap: "wrap",
            gap: "6px 18px",
            alignItems: "center",
            background: "var(--surface-2)",
          }}
        >
          <strong style={{ fontSize: 13.5 }}>
            {prettyAppName(selected.name, selected.source)}
          </strong>
          <span className="ell" style={{ color: "var(--muted)", maxWidth: 280 }} title={selected.publisher}>
            {L.detailPublisher}: {selected.publisher || "—"}
          </span>
          <span style={{ color: "var(--muted)", fontFamily: "var(--mono)" }}>
            {L.detailVersion}: {selected.version || "—"}
          </span>
          <span style={{ color: "var(--muted)", fontFamily: "var(--mono)" }}>
            {L.detailDate}: {selected.install_date || "—"}
          </span>
          <span style={css.sourceBadge}>{selected.source}</span>
          <span
            className="ell"
            style={{ color: "var(--muted)", fontFamily: "var(--mono)", flex: "1 1 200px", minWidth: 0 }}
            title={selected.install_location}
          >
            {selected.install_location || "—"}
          </span>
        </div>
      )}

      {postUninstallApp && (
        <div
          style={{
            ...css.card,
            marginBottom: 10,
            padding: "12px 14px",
            display: "flex",
            gap: 12,
            alignItems: "center",
            flexWrap: "wrap",
            fontSize: 13,
          }}
        >
          <span style={{ flex: "1 1 220px" }}>
            {officialResult?.ok ? L.uninstallOk : officialResult ? `${L.uninstallFail}: ${officialResult.message}` : L.uninstallOk}
            {" — "}
            {L.leftoversPrompt}
          </span>
          <button
            style={css.btn}
            disabled={scanning}
            onClick={() => void scanLeftoversFor(postUninstallApp)}
          >
            {scanning ? L.analyzing : L.scanLeftovers}
          </button>
          <button style={css.btnGhost} onClick={() => void skipLeftovers()}>
            {L.skipLeftovers}
          </button>
        </div>
      )}

      {scan && (
        <div
          style={{
            ...css.toolbar,
            marginBottom: 10,
            padding: "8px 12px",
            background: "var(--surface-2)",
            border: "1px solid var(--border)",
            borderRadius: 8,
          }}
        >
          <button
            style={css.btnGhost}
            onClick={() => {
              setScan(null);
              setReport(null);
              setError(null);
              void refreshApps();
            }}
          >
            ← {L.closePreview}
          </button>
          <strong style={{ fontSize: 13, fontWeight: 600 }}>{scan.app_name}</strong>
          <span style={{ ...css.muted }}>
            {scan.items.length} · {scan.items.filter((i) => i.confidence === "confirmed").length} ★
          </span>
          {!postUninstallApp && (
            <label
              style={{
                fontSize: 12.5,
                display: "flex",
                alignItems: "center",
                gap: 6,
                color: "var(--muted)",
              }}
              title={L.batchOfficialHint}
            >
              <input
                type="checkbox"
                checked={useOfficial}
                onChange={(e) => setUseOfficial(e.target.checked)}
                style={{ accentColor: "var(--accent)" }}
              />
              {L.useOfficial}
            </label>
          )}
          <button style={css.btnGhost} disabled={dryRunning || selectedPaths.size === 0} onClick={dryRun}>
            {L.dryRun}
          </button>
          <button
            style={{ ...css.btn, background: "var(--danger)", color: "#1a0505" }}
            disabled={dryRunning || selectedPaths.size === 0 || busyRef.current}
            onClick={() => {
              if (!window.confirm(L.cleanupConfirm(selectedPaths.size, !postUninstallApp && useOfficial))) return;
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
          {"restore_point_ok" in report && (
            <div style={{ marginTop: 4, color: "var(--muted)", fontSize: 12 }}>
              {report.restore_point_ok ? L.restorePointOk : L.restorePointFail}
              {report.restore_point_msg ? ` — ${report.restore_point_msg}` : ""}
            </div>
          )}
        </div>
      )}

      {batching && batchTotal > 0 && (
        <BatchProgress index={batchIndex} total={batchTotal} current={batchCurrent} />
      )}

      {showBatchSummary && batchResults.length > 0 && (
        <BatchSummaryPanel
          results={batchResults}
          onRetryFailed={retryFailedBatch}
          onDismiss={() => setShowBatchSummary(false)}
        />
      )}

      {scan ? (
        <div style={{ ...css.card, flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}>
          <div style={{ padding: "12px 16px", borderBottom: "1px solid var(--border)", flexShrink: 0 }}>
            <strong>{scan.app_name}</strong>
            <span style={{ ...css.muted, marginLeft: 12 }}>
              {L.leftoversTitle}: {scan.items.length} ·{" "}
              {scan.items.filter((i) => i.confidence === "confirmed").length} {L.confirmed}
            </span>
          </div>
          <div style={css.scroll}>
            <table style={css.table}>
              <thead>
                <tr>
                  <th style={css.th}>✓</th>
                  <th style={css.th}>{L.colLocation}</th>
                  <th style={css.th}>{L.colSource}</th>
                  <th style={css.th}>{L.confirmed}</th>
                  <th style={css.th}>ⓘ</th>
                </tr>
              </thead>
              <tbody>
                {scan.items.length === 0 && (
                  <tr>
                    <td colSpan={5} style={{ ...css.td, color: "var(--muted)" }}>
                      {L.leftoversNone}
                    </td>
                  </tr>
                )}
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
                    <td style={css.td}>
                      <span className="ell" style={{ display: "block" }} title={it.path}>
                        {it.path}
                      </span>
                    </td>
                    <td style={css.td}>
                      <span style={css.sourceBadge}>{it.kind}</span>
                    </td>
                    <td style={css.td}>
                      <span
                        style={{
                          color:
                            it.risk === "high"
                              ? "var(--danger)"
                              : it.confidence === "confirmed"
                                ? "var(--ok)"
                                : "var(--warn)",
                          fontWeight: 600,
                          fontSize: 12,
                        }}
                      >
                        {it.confidence === "confirmed"
                          ? L.confirmed
                          : it.score >= 30
                            ? L.suspected
                            : L.low}
                        {it.risk === "high" ? " · high" : ""}
                      </span>
                    </td>
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
                flexShrink: 0,
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
        <div style={{ ...css.card, flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}>
          <div ref={listScrollRef} style={css.scroll}>
            <table style={css.table}>
              <colgroup>
                <col style={{ width: 40 }} />
                <col />
                <col style={{ width: 100 }} />
                <col style={{ width: 150 }} />
              </colgroup>
              <thead>
                <tr>
                  <th style={css.th}>✓</th>
                  <th
                    style={{ ...css.th, cursor: "pointer" }}
                    onClick={() => sortBy("name")}
                    title={L.colName}
                  >
                    {L.colName} {sortCol === "name" ? (sortDesc ? "↓" : "↑") : ""}
                  </th>
                  <th
                    style={{ ...css.th, cursor: "pointer", textAlign: "right" as const }}
                    onClick={() => sortBy("size")}
                  >
                    {L.colSize} {sortCol === "size" ? (sortDesc ? "↓" : "↑") : ""}
                  </th>
                  <th style={{ ...css.th, textAlign: "right" as const }}>{L.actionCol}</th>
                </tr>
              </thead>
              <tbody>
                {filtered.length === 0 && (
                  <tr>
                    <td colSpan={4} style={{ ...css.td, color: "var(--muted)", textAlign: "center" as const, padding: 28 }}>
                      {loading ? "…" : L.emptyList}
                    </td>
                  </tr>
                )}
                {virtualRows.length > 0 ? (
                  <>
                    {virtualRows[0].start > 0 && (
                      <tr aria-hidden style={{ height: virtualRows[0].start }}>
                        <td colSpan={4} style={{ padding: 0, border: "none" }} />
                      </tr>
                    )}
                    {virtualRows.map((vr) => {
                      const a = filtered[vr.index];
                      const key = appKey(a);
                      return (
                        <AppRow
                          key={key}
                          app={a}
                          rowKey={key}
                          selected={
                            selected?.registry_key === a.registry_key && selected.name === a.name
                          }
                          checked={multi.has(key)}
                          sizeText={formatAppSize(a)}
                          uninstalling={uninstallingKey === key}
                          onSelect={setSelected}
                          onUninstall={(app) => void startUninstall(app)}
                          onAnalyze={(app) => void analyze(app)}
                          onForceClean={(app) => void forceClean(app)}
                          onIgnoreApp={(app) => void doIgnoreApp(app)}
                          onIgnorePub={(app) => void doIgnorePublisher(app)}
                          onToggleMulti={toggleMulti}
                          onEnsureSelected={(app) => {
                            if (!selected) setSelected(app);
                          }}
                        />
                      );
                    })}
                    {(() => {
                      const last = virtualRows[virtualRows.length - 1];
                      const pad = totalSize - last.end;
                      return pad > 0 ? (
                        <tr aria-hidden style={{ height: pad }}>
                          <td colSpan={4} style={{ padding: 0, border: "none" }} />
                        </tr>
                      ) : null;
                    })()}
                  </>
                ) : (
                  filtered.map((a) => {
                    const key = appKey(a);
                    return (
                      <AppRow
                        key={key}
                        app={a}
                        rowKey={key}
                        selected={
                          selected?.registry_key === a.registry_key && selected.name === a.name
                        }
                        checked={multi.has(key)}
                        sizeText={formatAppSize(a)}
                        uninstalling={uninstallingKey === key}
                        onSelect={setSelected}
                        onUninstall={(app) => void startUninstall(app)}
                        onAnalyze={(app) => void analyze(app)}
                        onForceClean={(app) => void forceClean(app)}
                        onIgnoreApp={(app) => void doIgnoreApp(app)}
                        onIgnorePub={(app) => void doIgnorePublisher(app)}
                        onToggleMulti={toggleMulti}
                        onEnsureSelected={(app) => {
                          if (!selected) setSelected(app);
                        }}
                      />
                    );
                  })
                )}
              </tbody>
            </table>
          </div>
          {multi.size > 0 && !scan && (
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 12,
                padding: "12px 14px",
                borderTop: "1px solid var(--border)",
                background: "var(--surface-2)",
                flexShrink: 0,
              }}
            >
              <span style={{ fontSize: 13, fontWeight: 600 }}>
                {L.batchUninstall} · {multi.size}
              </span>
              <button
                style={css.btnGhost}
                onClick={() => (batching ? cancelBatch() : setMulti(new Set()))}
              >
                {batching ? L.batchCancel : L.batchDismiss}
              </button>
              <button
                style={{ ...css.btn, marginLeft: "auto", background: "var(--danger)", color: "#fff" }}
                disabled={batching}
                title={L.batchOfficialHint}
                onClick={() => void batchCleanup()}
              >
                {`${L.batchUninstall} (${multi.size})`}
              </button>
            </div>
          )}
        </div>
      )}
          </>
        )}
      </Shell>
    </>
  );
}
