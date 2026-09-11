import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { InstalledApp } from "./types";

const styles = {
  page: { padding: 24, maxWidth: 1280, margin: "0 auto" },
  header: { display: "flex", alignItems: "baseline", gap: 16, marginBottom: 16 },
  h1: { fontSize: 28, fontWeight: 700, margin: 0 },
  muted: { color: "#2f3e52", fontSize: 14 },
  bar: {
    display: "flex",
    gap: 12,
    marginBottom: 12,
    alignItems: "center",
  },
  input: {
    flex: 1,
    height: 40,
    borderRadius: 10,
    border: "1px solid #d0d7e2",
    padding: "0 14px",
    fontSize: 14,
    background: "#fff",
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
  scroll: { maxHeight: "calc(100vh - 180px)", overflow: "auto" as const },
};

export default function App() {
  const [apps, setApps] = useState<InstalledApp[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [q, setQ] = useState("");

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

  return (
    <div style={styles.page}>
      <div style={styles.header}>
        <h1 style={styles.h1}>Remova</h1>
        <span style={styles.muted}>Deep Uninstall · Phase 0（只读列表）</span>
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
      </div>
      {error && (
        <div style={{ color: "#b91c1c", marginBottom: 12 }}>{error}</div>
      )}
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
                <tr key={a.registry_key + a.name}>
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
    </div>
  );
}
