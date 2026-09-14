import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  CleanupReport,
  FullCleanupReport,
  InstalledApp,
  ScanResult,
} from "./types";
import { currentLang, formatSize, loadLang, setLang, t } from "./i18n";
import { compareSemver } from "./semver";
import { cssStyles as css, globalCss } from "./styles";
import { HistoryPanel } from "./components/HistoryPanel";
import { RestorePanel } from "./components/RestorePanel";
import { MonitorPanel } from "./components/MonitorPanel";
import { ManagePanel, type ManageItem, type ManageTab } from "./components/ManagePanel";
import { AppRow } from "./components/AppRow";
import {
  BatchProgress,
  BatchSummaryPanel,
  type BatchItemResult,
} from "./components/BatchPanels";
import { escapeHtml, formatError } from "./lib/format";
import { applyTheme, loadTheme, type Theme } from "./lib/theme";

declare const __APP_VERSION__: string;

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
      setNotice(`${L.forceClean}: ${selected.name} · ${L.batchDetail(report.deleted, report.failed)}`);
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
      setScan({
        app_name: L.orphanScan,
        items,
      });
      setSelectedPaths(new Set(items.filter((i) => i.confidence === "confirmed").map((i) => i.path)));
      setNotice(items.length === 0 ? L.orphanScanEmpty : `${L.orphanScan}: ${items.length}`);
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
          {showTools ? L.toolsCollapse : L.toolsExpand}
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
          {"restore_point_ok" in report && (
            <div style={{ marginTop: 4, color: "var(--muted)", fontSize: 12 }}>
              {report.restore_point_ok ? L.restorePointOk : L.restorePointFail}
              {report.restore_point_msg ? ` — ${report.restore_point_msg}` : ""}
            </div>
          )}
        </div>
      )}

      {showHistory && (
        <HistoryPanel
          history={history}
          histQ={histQ}
          setHistQ={setHistQ}
          onClose={() => setShowHistory(false)}
        />
      )}

      {showRestore && (
        <RestorePanel
          sessions={restoreSessions}
          pick={restorePick}
          setPick={setRestorePick}
          busy={restoreBusy}
          msgs={restoreMsgs}
          onRun={() => void runRestoreSession()}
          onClose={() => setShowRestore(false)}
        />
      )}

      {monitorDiff && (
        <MonitorPanel diff={monitorDiff} onDismiss={() => setMonitorDiff(null)} />
      )}

      {showManage && (
        <ManagePanel
          tab={manageTab}
          items={manageItems}
          busy={manageBusy}
          onTab={(tab) => void loadManage(tab)}
          onReload={() => void loadManage(manageTab)}
          onToggle={(it) => void toggleManageItem(it)}
          onClose={() => setShowManage(false)}
        />
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
                    <AppRow
                      key={key}
                      app={a}
                      rowKey={key}
                      selected={
                        selected?.registry_key === a.registry_key && selected.name === a.name
                      }
                      checked={multi.has(key)}
                      sizeText={formatAppSize(a)}
                      onSelect={setSelected}
                      onAnalyze={analyze}
                      onToggleMulti={toggleMulti}
                      onEnsureSelected={(app) => {
                        if (!selected) setSelected(app);
                      }}
                    />
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
