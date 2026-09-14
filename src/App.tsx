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
    root.style.setProperty("--bg", "#0f1419");
    root.style.setProperty("--fg", "#e6edf3");
    root.style.setProperty("--muted", "#9ba7b4");
    root.style.setProperty("--surface", "#1c2128");
    root.style.setProperty("--border", "#30363d");
    root.style.setProperty("--accent", "#14b8a6");
    root.style.setProperty("--th-bg", "#262c36");
  } else {
    root.style.setProperty("--bg", "#f5f7fa");
    root.style.setProperty("--fg", "#0b1220");
    root.style.setProperty("--muted", "#2f3e52");
    root.style.setProperty("--surface", "#ffffff");
    root.style.setProperty("--border", "#d0d7e2");
    root.style.setProperty("--accent", "#0d9488");
    root.style.setProperty("--th-bg", "#e8eef5");
  }
  localStorage.setItem("remova_theme", theme);
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

export default function App() {
  const [apps, setApps] = useState<InstalledApp[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const [selected, setSelected] = useState<InstalledApp | null>(null);
  const [multi, setMulti] = useState<Set<string>>(new Set());
  const [scan, setScan] = useState<ScanResult | null>(null);
  const [scanning, setScanning] = useState(false);
  const [, setReport] = useState<CleanupReport | FullCleanupReport | null>(null);
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

  const L = useMemo(() => t(), [langVer]);

  const filtered = useMemo(() => {
    const needle = q.trim().toLowerCase();
    let list = apps;
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
  }, [apps, q, sortCol, sortDesc, sizeOf, sizeMap]);

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
    } catch (e) {
      setError(formatError(e, "cleanup"));
    } finally {
      busyRef.current = false;
      setDryRunning(false);
    }
  }, [scan, selected, useOfficial, selectedPaths]);

  const batchCleanup = useCallback(async () => {
    busyRef.current = true;
    const keys = new Set(multi);
    const queue = apps.filter((a) => keys.has(a.registry_key + a.name));
    if (!queue.length) return;
    if (!window.confirm(L.batchConfirm(queue.length))) return;
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
      // Keep failures selected for retry; drop successes from multi.
      setMulti((m) => {
        const n = new Set(m);
        for (const k of okKeys) n.delete(k);
        return n;
      });
      setShowBatchSummary(true);
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
    page: { padding: 24, maxWidth: 1280, margin: "0 auto", color: "var(--fg)" },
    muted: { color: "var(--muted)", fontSize: 14 },
    input: {
      flex: 1,
      height: 40,
      borderRadius: 10,
      border: "1px solid var(--border)",
      padding: "0 14px",
      fontSize: 14,
      background: "var(--surface)",
      color: "var(--fg)",
    },
    btn: {
      height: 40,
      padding: "0 14px",
      borderRadius: 10,
      border: "none",
      background: "var(--accent)",
      color: "#fff",
      fontWeight: 600,
      cursor: "pointer",
    },
    btnGhost: {
      height: 40,
      padding: "0 14px",
      borderRadius: 10,
      border: "1px solid var(--border)",
      background: "var(--surface)",
      color: "var(--fg)",
      cursor: "pointer",
    },
    card: {
      background: "var(--surface)",
      border: "1px solid var(--border)",
      borderRadius: 16,
      overflow: "hidden",
    },
    th: {
      textAlign: "left" as const,
      padding: "10px 12px",
      background: "var(--th-bg)",
      borderBottom: "1px solid var(--border)",
      position: "sticky" as const,
      top: 0,
    },
    td: {
      padding: "10px 12px",
      borderBottom: "1px solid var(--border)",
      verticalAlign: "top" as const,
    },
    table: { width: "100%", borderCollapse: "collapse" as const, fontSize: 13 },
    scroll: { maxHeight: "calc(100vh - 240px)", overflow: "auto" as const },
  };

  return (
    <div style={css.page}>
      <div style={{ display: "flex", gap: 16, alignItems: "baseline", marginBottom: 16 }}>
        <h1 style={{ fontSize: 28, fontWeight: 700, margin: 0 }}>{L.title}</h1>
        <span style={css.muted}>{L.subtitle}</span>
        <span style={{ ...css.muted, marginLeft: "auto" }}>
          {loading ? "…" : `${filtered.length} / ${apps.length}`}
          {admin === false && ` · ${L.nonAdmin}`}
          {admin === true && ` · ${L.admin}`}
          {disk && ` · ${L.disk} ${disk}`}
        </span>
        <button
          style={css.btnGhost}
          onClick={() => setTheme((th) => (th === "dark" ? "light" : "dark"))}
        >
          {L.themeToggle}
        </button>
        <button
          style={css.btnGhost}
          onClick={() => {
            const next = currentLang() === "zh" ? "en" : "zh";
            setLang(next);
            setLangVer((v) => v + 1);
          }}
        >
          {L.langToggle}
        </button>
      </div>

      {showGuide && (
        <div
          style={{
            ...css.card,
            padding: 12,
            marginBottom: 12,
            background: "var(--th-bg)",
            fontSize: 13,
          }}
        >
          {L.guided}{" "}
          <button
            style={{ ...css.btnGhost, height: 28 }}
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
        <div style={{ marginBottom: 8, fontSize: 13, color: "var(--muted)" }}>
          {notice}
          {estimating ? ` · ${L.estimatingSizes}` : ""}
        </div>
      )}

      <div style={{ display: "flex", gap: 10, marginBottom: 12, flexWrap: "wrap" }}>
        <input
          style={css.input}
          placeholder={L.search}
          value={q}
          onChange={(e) => setQ(e.target.value)}
        />
        <button
          style={css.btnGhost}
          onClick={async () => {
            const h = await invoke<
              { app_name: string; deleted: number; failed: number; backup_dir: string }[]
            >("list_cleanup_history");
            setHistory(h);
            setShowHistory(true);
          }}
        >
          {L.history}
        </button>
        <button
          style={css.btnGhost}
          onClick={async () => {
            const csv = await invoke<string>("export_history_csv");
            const blob = new Blob([csv], { type: "text/csv;charset=utf-8" });
            const url = URL.createObjectURL(blob);
            const a = document.createElement("a");
            a.href = url;
            a.download = "remova-history.csv";
            a.click();
            URL.revokeObjectURL(url);
          }}
        >
          {L.exportCsv}
        </button>
        <button
          style={css.btnGhost}
          onClick={async () => {
            try {
              const names = await invoke<string[]>("list_restore_sessions");
              if (names.length === 0) {
                setNotice("no backup sessions");
                return;
              }
              const pick = window.prompt(
                `Sessions:\n${names.slice(0, 15).join("\n")}\n\nEnter session name to restore:`,
                names[0],
              );
              if (!pick) return;
              if (!window.confirm(L.restoreConfirm)) return;
              const msgs = await invoke<string[]>("restore_session_by_name", { name: pick });
              alert(msgs.slice(0, 8).join("\n") || "ok");
            } catch (e) {
              setError(formatError(e));
            }
          }}
        >
          {L.restore}
        </button>
        {batching ? (
          <button style={{ ...css.btn, background: "#b45309" }} onClick={() => cancelBatch()}>
            {L.batchCancel}
          </button>
        ) : (
          <button
            style={{ ...css.btn, opacity: multi.size === 0 ? 0.5 : 1 }}
            disabled={multi.size === 0}
            onClick={() => void batchCleanup()}
          >
            {L.batch} ({multi.size})
          </button>
        )}
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
        <button
          style={{
            ...css.btn,
            opacity: !analyzeApp || scanning ? 0.5 : 1,
            cursor: !analyzeApp || scanning ? "not-allowed" : "pointer",
          }}
          disabled={!analyzeApp || scanning}
          title={!analyzeApp ? L.selectRowHint : undefined}
          onClick={() => analyzeApp && analyze(analyzeApp)}
        >
          {scanning ? L.analyzing : L.analyze}
        </button>
        {estimating && (
          <button style={css.btnGhost} onClick={() => void stopSizeEstimate()}>
            {L.stopEstimate}
          </button>
        )}
        {scan && (
          <>
            <label style={{ fontSize: 13, display: "flex", alignItems: "center", gap: 6 }}>
              <input
                type="checkbox"
                checked={useOfficial}
                onChange={(e) => setUseOfficial(e.target.checked)}
              />
              {L.useOfficial}
            </label>
            <button
              style={{ ...css.btn, background: "#b91c1c" }}
              disabled={dryRunning || selectedPaths.size === 0}
              onClick={() => {
                if (!window.confirm(L.cleanupConfirm(selectedPaths.size, useOfficial))) return;
                void execReal();
              }}
            >
              {L.cleanup} ({selectedPaths.size})
            </button>
            <button style={css.btnGhost} disabled={dryRunning || selectedPaths.size === 0} onClick={dryRun}>
              {L.dryRun}
            </button>
            <button
              style={css.btnGhost}
              onClick={() => {
                setScan(null);
                setReport(null);
              }}
            >
              {L.closePreview}
            </button>
          </>
        )}
      </div>

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
                        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                          <AppIcon displayIcon={a.display_icon} name={a.name} />
                          <span>{a.name}</span>
                        </div>
                      </td>
                      <td style={css.td}>{a.version || "—"}</td>
                      <td style={css.td}>{a.publisher || "—"}</td>
                      <td style={{ ...css.td, textAlign: "right" as const, whiteSpace: "nowrap" }}>
                        {formatAppSize(a)}
                      </td>
                      <td style={{ ...css.td, whiteSpace: "nowrap" }}>{a.install_date || "—"}</td>
                      <td style={css.td}>{a.source}</td>
                      <td style={css.td}>{a.install_location || "—"}</td>
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
