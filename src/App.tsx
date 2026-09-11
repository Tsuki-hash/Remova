import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { InstalledApp, ScanResult } from "./types";

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

  return (
    <div style={styles.page}>
      <div style={styles.header}>
        <h1 style={styles.h1}>Remova</h1>
        <span style={styles.muted}>Deep Uninstall · Phase 1（只读关联分析）</span>
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
        <button
          style={styles.btn}
          disabled={!selected || scanning}
          onClick={() => selected && analyze(selected)}
        >
          {scanning ? "分析中…" : "深度分析"}
        </button>
        {scan && (
          <button style={styles.btnGhost} onClick={() => setScan(null)}>
            关闭预览
          </button>
        )}
      </div>
      {error && <div style={{ color: "#b91c1c", marginBottom: 12 }}>{error}</div>}

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
