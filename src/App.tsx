import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { CleanupReport, FullCleanupReport, InstalledApp, ScanResult } from "./types";

const styles = {
  page: { padding: 24, maxWidth: 1280, margin: "0 auto" },
  header: { display: "flex", alignItems: "baseline", gap: 16, marginBottom: 16 },
  h1: { fontSize: 28, fontWeight: 700, margin: 0 },
  muted: { color: "#2f3e52", fontSize: 14 },
  bar: { display: "flex", gap: 12, marginBottom: 12, alignItems: "center" },
  input: {
    flex: 1,
    height: 40,
    borderRadius: 10,
    border: "1px solid #d0d7e2",
    padding: "0 14px",
    fontSize: 14,
    background: "#fff",
  },
  btn: {
    height: 40,
    padding: "0 16px",
    borderRadius: 10,
    border: "none",
    background: "#0d9488",
    color: "#fff",
    fontWeight: 600,
    cursor: "pointer",
  },
  btnGhost: {
    height: 40,
    padding: "0 16px",
    borderRadius: 10,
    border: "1px solid #d0d7e2",
    background: "#fff",
    color: "#0b1220",
    cursor: "pointer",
  },
  card: {
    background: "#fff",
    border: "1px solid #d0d7e2",
    borderRadius: 16,
    overflow: "hidden",
  },
  table: { width: "100%", borderCollapse: "collapse" as const, fontSize: 13 },
  th: {
    textAlign: "left" as const,
    padding: "10px 12px",
    background: "#e8eef5",
    borderBottom: "1px solid #d0d7e2",
    position: "sticky" as const,
    top: 0,
  },
  td: {
    padding: "10px 12px",
    borderBottom: "1px solid #e8eef5",
    verticalAlign: "top" as const,
  },
  scroll: { maxHeight: "calc(100vh - 200px)", overflow: "auto" as const },
  confConfirmed: { color: "#0f766e", fontWeight: 600 },
  confSuspected: { color: "#b45309" },
  riskHigh: { color: "#b91c1c", fontWeight: 600 },
};

export default function App() {
  const [apps, setApps] = useState<InstalledApp[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const [selected, setSelected] = useState<InstalledApp | null>(null);
  const [scan, setScan] = useState<ScanResult | null>(null);
  const [scanning, setScanning] = useState(false);
  const [report, setReport] = useState<CleanupReport | FullCleanupReport | null>(null);
  const [dryRunning, setDryRunning] = useState(false);
  const [useOfficial, setUseOfficial] = useState(false);
  const [history, setHistory] = useState<
    { app_name: string; deleted: number; failed: number; backup_dir: string }[]
  >([]);
  const [showHistory, setShowHistory] = useState(false);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const list = await invoke<InstalledApp[]>("list_installed_apps");
        if (!cancelled) {
          setApps(list);
          setError(null);
        }
      } catch (e) {
        if (!cancelled) setError(String(e));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const filtered = useMemo(() => {
    const needle = q.trim().toLowerCase();
    if (!needle) return apps;
    return apps.filter(
      (a) =>
        a.name.toLowerCase().includes(needle) ||
        a.publisher.toLowerCase().includes(needle) ||
        a.install_location.toLowerCase().includes(needle),
    );
  }, [apps, q]);

  const analyze = useCallback(async (app: InstalledApp) => {
    setSelected(app);
    setScanning(true);
    setScan(null);
    setReport(null);
    try {
      const r = await invoke<ScanResult>("analyze_associations", { app });
      setScan(r);
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setScanning(false);
    }
  }, []);

  const dryRun = useCallback(async () => {
    if (!scan) return;
    setDryRunning(true);
    try {
      const r = await invoke<CleanupReport>("run_cleanup_dry_run", {
        appName: scan.app_name,
        items: scan.items,
      });
      setReport(r);
      setError(null);
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
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setDryRunning(false);
    }
  }, [scan, selected, useOfficial]);

  const restoreLatest = useCallback(async () => {
    if (!window.confirm("将从最近备份还原文件与注册表。继续？")) return;
    try {
      const msgs = await invoke<string[]>("restore_latest_backup");
      window.alert(msgs.length ? `还原完成：\n${msgs.slice(0, 8).join("\n")}` : "无内容可还原");
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const loadHistory = useCallback(async () => {
    try {
      const h = await invoke<
        { app_name: string; deleted: number; failed: number; backup_dir: string }[]
      >("list_cleanup_history");
      setHistory(h);
      setShowHistory(true);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  return (
    <div style={styles.page}>
      <div style={styles.header}>
        <h1 style={styles.h1}>Remova</h1>
        <span style={styles.muted}>Deep Uninstall · Tauri + React + Rust</span>
        <span style={{ ...styles.muted, marginLeft: "auto" }}>
          {loading ? "加载中…" : `${filtered.length} / ${apps.length} 个软件`}
        </span>
      </div>
      <div style={styles.bar}>
        <input
          style={styles.input}
          placeholder="搜索名称 / 发布者 / 安装路径…"
          value={q}
          onChange={(e) => setQ(e.target.value)}
        />
        <button style={styles.btnGhost} onClick={loadHistory}>
          历史
        </button>
        <button style={styles.btnGhost} onClick={restoreLatest}>
          还原最近备份
        </button>
        <button
          style={styles.btn}
          disabled={!selected || scanning}
          onClick={() => selected && analyze(selected)}
        >
          {scanning ? "分析中…" : "深度分析"}
        </button>
        {scan && (
          <>
            <label style={{ fontSize: 13, display: "flex", alignItems: "center", gap: 6 }}>
              <input
                type="checkbox"
                checked={useOfficial}
                onChange={(e) => setUseOfficial(e.target.checked)}
              />
              调用官方卸载器
            </label>
            <button style={styles.btn} disabled={dryRunning} onClick={dryRun}>
              {dryRunning ? "处理中…" : "演练清理"}
            </button>
            <button
              style={{ ...styles.btn, background: "#b91c1c", opacity: dryRunning ? 0.6 : 1 }}
              disabled={dryRunning}
              onClick={() => {
                if (
                  !window.confirm(
                    `将备份并清理 ${scan.items.length} 项${useOfficial ? "并调用官方卸载器" : ""}。确认？`,
                  )
                ) {
                  return;
                }
                void execReal();
              }}
            >
              备份并清理
            </button>
            <button
              style={styles.btnGhost}
              onClick={() => {
                setScan(null);
                setReport(null);
              }}
            >
              关闭预览
            </button>
          </>
        )}
      </div>
      {showHistory && (
        <div style={{ ...styles.card, marginBottom: 12, padding: 12, fontSize: 13 }}>
          <strong>清理历史</strong>
          <button style={{ marginLeft: 12, ...styles.btnGhost }} onClick={() => setShowHistory(false)}>
            关闭
          </button>
          <div style={{ maxHeight: 160, overflow: "auto", marginTop: 8 }}>
            {history.length === 0 && <div>暂无记录</div>}
            {history.map((h, i) => (
              <div key={i}>
                {h.app_name} · 删 {h.deleted} · 失败 {h.failed}
                {h.backup_dir ? ` · ${h.backup_dir}` : ""}
              </div>
            ))}
          </div>
        </div>
      )}
      {error && <div style={{ color: "#b91c1c", marginBottom: 12 }}>{error}</div>}
      {report && (
        <div
          style={{
            marginBottom: 12,
            padding: "12px 16px",
            background: "#e8eef5",
            borderRadius: 12,
            fontSize: 13,
          }}
        >
          <strong>{report.dry_run ? "dry-run 摘要" : "清理摘要"}</strong>
          <span style={{ marginLeft: 12 }}>
            {report.dry_run
              ? `计划删除 ${(report as CleanupReport).deleted_planned} · 跳过 ${report.skipped} · ${report.uninstall_message}`
              : `已删除 ${(report as FullCleanupReport).deleted} · 失败 ${(report as FullCleanupReport).failed} · 跳过 ${report.skipped} · ${(report as FullCleanupReport).aborted ? "已中止 · " : ""}${report.uninstall_message}${(report as FullCleanupReport).backup_dir ? " · 备份 " + (report as FullCleanupReport).backup_dir : ""}`}
          </span>
          {report.item_details.length > 0 && (
            <div style={{ marginTop: 8, maxHeight: 120, overflow: "auto" }}>
              {report.item_details.slice(0, 20).map((d) => (
                <div key={d.path}>
                  [{d.status}] {d.path}
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {scan ? (
        <div style={styles.card}>
          <div style={{ padding: "12px 16px", borderBottom: "1px solid #e8eef5" }}>
            <strong>{scan.app_name}</strong>
            <span style={{ ...styles.muted, marginLeft: 12 }}>
              共 {scan.items.length} 项 · 确定{" "}
              {scan.items.filter((i) => i.confidence === "confirmed").length} · 疑似{" "}
              {scan.items.filter((i) => i.confidence === "suspected").length}
            </span>
          </div>
          <div style={styles.scroll}>
            <table style={styles.table}>
              <thead>
                <tr>
                  <th style={styles.th}>路径 / 键</th>
                  <th style={styles.th}>类型</th>
                  <th style={styles.th}>分数</th>
                  <th style={styles.th}>关联</th>
                  <th style={styles.th}>风险</th>
                  <th style={styles.th}>依据</th>
                </tr>
              </thead>
              <tbody>
                {scan.items.map((it) => (
                  <tr key={it.path}>
                    <td style={styles.td}>{it.path}</td>
                    <td style={styles.td}>{it.kind}</td>
                    <td style={styles.td}>{it.score}</td>
                    <td
                      style={{
                        ...styles.td,
                        ...(it.confidence === "confirmed"
                          ? styles.confConfirmed
                          : styles.confSuspected),
                      }}
                    >
                      {it.confidence === "confirmed" ? "确定" : "疑似"}
                    </td>
                    <td
                      style={{
                        ...styles.td,
                        ...(it.risk === "high" ? styles.riskHigh : null),
                      }}
                    >
                      {it.risk}
                    </td>
                    <td style={styles.td}>
                      {it.evidence.map((e) => (
                        <div key={e.code + e.detail}>
                          {e.label} ({e.weight})
                          {e.detail ? ` — ${e.detail.slice(0, 80)}` : ""}
                        </div>
                      ))}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      ) : (
        <div style={styles.card}>
          <div style={styles.scroll}>
            <table style={styles.table}>
              <thead>
                <tr>
                  <th style={styles.th}>名称</th>
                  <th style={styles.th}>版本</th>
                  <th style={styles.th}>发布者</th>
                  <th style={styles.th}>来源</th>
                  <th style={styles.th}>安装路径</th>
                </tr>
              </thead>
              <tbody>
                {filtered.map((a) => (
                  <tr
                    key={a.registry_key + a.name}
                    onClick={() => setSelected(a)}
                    style={{
                      cursor: "pointer",
                      background:
                        selected?.registry_key === a.registry_key && selected.name === a.name
                          ? "#ccfbf1"
                          : undefined,
                    }}
                    onDoubleClick={() => analyze(a)}
                  >
                    <td style={styles.td}>{a.name}</td>
                    <td style={styles.td}>{a.version || "—"}</td>
                    <td style={styles.td}>{a.publisher || "—"}</td>
                    <td style={styles.td}>{a.source}</td>
                    <td style={styles.td}>{a.install_location || "—"}</td>
                  </tr>
                ))}
                {!loading && filtered.length === 0 && (
                  <tr>
                    <td style={styles.td} colSpan={5}>
                      无匹配项
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  );
}
