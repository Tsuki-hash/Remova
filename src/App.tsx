import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  CleanupReport,
  FullCleanupReport,
  InstalledApp,
  ScanResult,
} from "./types";
import { currentLang, loadLang, setLang, t } from "./i18n";

type Theme = "light" | "dark";

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
  const [sortCol, setSortCol] = useState<"name" | "publisher" | "install_location" | null>(null);
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
      setError(null);
      setNotice(`analyze ${(performance.now() - t0) / 1000}s · ${r.items.length} items`);
    } catch (e) {
      setError(String(e));
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
        if (!cancelled) setError(String(e));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    void invoke<boolean>("is_elevated").then(setAdmin).catch(() => {});
    void invoke<{ free_gb: number; total_gb: number }>("disk_usage")
      .then((d) => setDisk(`C: ${d.free_gb.toFixed(1)} / ${d.total_gb.toFixed(0)} GB`))
      .catch(() => {});
    void fetch("https://api.github.com/repos/Tsuki-hash/Remova-next/releases/latest", {
      headers: { Accept: "application/vnd.github+json" },
    })
      .then((r) => (r.ok ? r.json() : null))
      .then((j: { tag_name?: string } | null) => {
        if (j?.tag_name) setNotice(`${t().versionNew}: ${j.tag_name}`);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
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
    const s = [...list].sort((a, b) =>
      (a[sortCol] || "").toLowerCase().localeCompare((b[sortCol] || "").toLowerCase()),
    );
    return sortDesc ? s.reverse() : s;
  }, [apps, q, sortCol, sortDesc]);

  const sortBy = (col: "name" | "publisher" | "install_location") => {
    if (sortCol === col) setSortDesc((d) => !d);
    else {
      setSortCol(col);
      setSortDesc(false);
    }
  };

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
    setDryRunning(true);
    try {
      const r = await invoke<CleanupReport>("run_cleanup_dry_run", {
        appName: scan.app_name,
        items: scan.items,
      });
      setReport(r);
    } catch (e) {
      setError(String(e));
    } finally {
      setDryRunning(false);
    }
  }, [scan]);

  const execReal = useCallback(async () => {
    if (!scan || !selected) return;
    setDryRunning(true);
    try {
      const r = await invoke<FullCleanupReport>("run_full_cleanup", {
        app: selected,
        items: scan.items,
        options: {
          dry_run: false,
          skip_official_uninstall: !useOfficial,
          backup_enabled: true,
        },
      });
      setReport(r);
    } catch (e) {
      setError(String(e));
    } finally {
      setDryRunning(false);
    }
  }, [scan, selected, useOfficial]);

  const batchCleanup = useCallback(async () => {
    const keys = new Set(multi);
    const queue = apps.filter((a) => keys.has(a.registry_key + a.name));
    if (!queue.length) return;
    if (!window.confirm(L.batchConfirm(queue.length))) return;
    setBatching(true);
    try {
      for (let i = 0; i < queue.length; i++) {
        const app = queue[i];
        setNotice(`[${i + 1}/${queue.length}] ${app.name}`);
        const r = await invoke<ScanResult>("analyze_associations", { app });
        const items = r.items.filter(
          (it) => it.confidence === "confirmed" && it.risk !== "high",
        );
        if (!items.length) continue;
        await invoke<FullCleanupReport>("run_full_cleanup", {
          app,
          items,
          options: {
            dry_run: false,
            skip_official_uninstall: true,
            backup_enabled: true,
          },
        });
      }
      setNotice("Batch finished");
      setMulti(new Set());
    } catch (e) {
      setError(String(e));
    } finally {
      setBatching(false);
    }
  }, [apps, multi, L]);

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
        <div style={{ marginBottom: 8, fontSize: 13, color: "var(--muted)" }}>{notice}</div>
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
            if (!window.confirm(L.restoreConfirm)) return;
            try {
              const msgs = await invoke<string[]>("restore_latest_backup");
              alert(msgs.slice(0, 8).join("\n") || "ok");
            } catch (e) {
              setError(String(e));
            }
          }}
        >
          {L.restore}
        </button>
        <button
          style={{ ...css.btn, opacity: batching ? 0.6 : 1 }}
          disabled={batching || multi.size === 0}
          onClick={() => void batchCleanup()}
        >
          {L.batch} ({multi.size})
        </button>
        <button
          style={css.btnGhost}
          disabled={admin === true || admin === null}
          onClick={async () => {
            try {
              await invoke("elevate_restart");
            } catch (e) {
              setError(String(e));
            }
          }}
        >
          Admin
        </button>
        <button
          style={css.btn}
          disabled={!selected || scanning}
          onClick={() => selected && analyze(selected)}
        >
          {scanning ? L.analyzing : L.analyze}
        </button>
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
            <button style={css.btn} disabled={dryRunning} onClick={dryRun}>
              {L.dryRun}
            </button>
            <button
              style={{ ...css.btn, background: "#b91c1c" }}
              disabled={dryRunning}
              onClick={() => {
                if (!window.confirm(L.cleanupConfirm(scan.items.length, useOfficial))) return;
                void execReal();
              }}
            >
              {L.cleanup}
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
          <button style={{ marginLeft: 12, ...css.btnGhost }} onClick={() => setShowHistory(false)}>
            ×
          </button>
          <div style={{ maxHeight: 160, overflow: "auto", marginTop: 8 }}>
            {history.length === 0 && <div>{L.noHistory}</div>}
            {history.map((h, i) => (
              <div key={i}>
                {h.app_name} · {h.deleted}/{h.failed}
                {h.backup_dir ? ` · ${h.backup_dir}` : ""}
              </div>
            ))}
          </div>
        </div>
      )}

      {error && <div style={{ color: "#b91c1c", marginBottom: 12 }}>{error}</div>}

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
                  <th style={css.th}>{L.colLocation}</th>
                  <th style={css.th}>type</th>
                  <th style={css.th}>score</th>
                  <th style={css.th}>match</th>
                  <th style={css.th}>risk</th>
                </tr>
              </thead>
              <tbody>
                {scan.items.map((it) => (
                  <tr key={it.path}>
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
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      ) : (
        <div style={css.card}>
          <div style={css.scroll}>
            <table style={css.table}>
              <thead>
                <tr>
                  <th style={css.th}>✓</th>
                  <th style={{ ...css.th, cursor: "pointer" }} onClick={() => sortBy("name")}>
                    {L.colName} {sortCol === "name" ? (sortDesc ? "↓" : "↑") : ""}
                  </th>
                  <th style={css.th}>{L.colVersion}</th>
                  <th
                    style={{ ...css.th, cursor: "pointer" }}
                    onClick={() => sortBy("publisher")}
                  >
                    {L.colPublisher} {sortCol === "publisher" ? (sortDesc ? "↓" : "↑") : ""}
                  </th>
                  <th style={css.th}>{L.colSource}</th>
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
                          onChange={() => toggleMulti(key)}
                          onClick={(e) => e.stopPropagation()}
                        />
                      </td>
                      <td style={css.td}>{a.name}</td>
                      <td style={css.td}>{a.version || "—"}</td>
                      <td style={css.td}>{a.publisher || "—"}</td>
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
