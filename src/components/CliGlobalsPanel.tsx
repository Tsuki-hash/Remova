import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { t, formatSize } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { toast } from "../lib/toast";
import { CloseGlyph } from "./ui/Glyph";
import type { CliGlobalItem, CliGlobalScanResult } from "../types";

const MANAGERS = ["npm", "pnpm", "yarn"] as const;

/** The directory a manager's globals live in, derived from a listed row. */
function rootOf(item: CliGlobalItem): string {
  const parts = item.path.replace(/[\\/]+$/, "").split(/[\\/]/);
  if (item.manager === "pnpm") {
    parts.pop();
    return parts.join("\\");
  }
  // npm/yarn rows are <prefix>\node_modules\(<pkg> | <scope>\<pkg>).
  const nodeModules = parts.lastIndexOf("node_modules");
  if (nodeModules > 0) return parts.slice(0, nodeModules).join("\\");
  return parts.join("\\");
}

/**
 * Read-only inventory of global CLI packages. Nothing here deletes — the
 * per-manager deletion gates land one at a time, npm first.
 */
export function CliGlobalsPanel({ onClose }: { onClose: () => void }) {
  const L = t();
  const [result, setResult] = useState<CliGlobalScanResult | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const r = await api.cliGlobals();
        if (alive) setResult(r);
      } catch (e) {
        if (alive) setLoadError(formatError(e));
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  const openRoot = async (item: CliGlobalItem) => {
    try {
      await api.openPath(rootOf(item));
    } catch (e) {
      toast.error(formatError(e));
    }
  };

  return (
    <section style={{ ...css.card, marginBottom: 16, padding: 16, fontSize: 13 }} aria-label={L.cliTitle}>
      <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap", marginBottom: 8 }}>
        <strong>{L.cliTitle}</strong>
        <span style={css.muted}>{L.cliHint}</span>
        <button
          style={{ ...css.btnGhost, height: 28, marginLeft: "auto" }}
          onClick={onClose}
          aria-label={L.panelClose}
        >
          <CloseGlyph />
        </button>
      </div>

      {loadError && (
        <div role="alert" style={{ color: "var(--danger-text)", marginBottom: 8 }}>
          {loadError}
        </div>
      )}

      {!result && !loadError && (
        <p role="status" style={css.muted}>
          {L.loadingGeneric}
        </p>
      )}

      {result && result.items.length === 0 && (
        <p style={css.muted}>{L.cliEmpty}</p>
      )}

      {result?.notes.map((m) => (
        <div key={m} style={{ ...css.muted, fontSize: 12 }}>
          {L.cliNoRoot(m)}
        </div>
      ))}

      {MANAGERS.map((m) => {
        const rows = result?.items.filter((i) => i.manager === m) ?? [];
        if (!rows.length || !result) return null;
        return (
          <div key={m} style={{ marginTop: 8 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <strong style={{ fontSize: 12 }}>{m}</strong>
              <span style={{ ...css.muted, fontSize: 12 }}>
                {rows.length} · {m === "pnpm" ? L.cliShimNote : ""}
              </span>
              <button
                type="button"
                style={{ ...css.btnGhost, height: 24, fontSize: 12, marginLeft: "auto" }}
                onClick={() => void openRoot(rows[0]!)}
              >
                {L.cliOpenRoot(m)}
              </button>
            </div>
            <div style={{ border: "1px solid var(--border)", borderRadius: 8, marginTop: 4, maxHeight: 200, overflow: "auto" }}>
              {rows.map((r) => (
                <div
                  key={r.path}
                  title={r.path}
                  style={{
                    display: "flex",
                    gap: 8,
                    padding: "3px 8px",
                    borderBottom: "1px solid var(--border)",
                    fontSize: 12,
                  }}
                >
                  <span className="ell" style={{ flex: 1, minWidth: 0 }}>
                    {r.name}
                  </span>
                  {r.version && <span style={css.mono}>{r.version}</span>}
                  {r.manager !== "pnpm" && (
                    <span style={{ ...css.mono, color: "var(--muted)" }}>{formatSize(r.size_kb)}</span>
                  )}
                </div>
              ))}
            </div>
          </div>
        );
      })}

      <div style={{ ...css.muted, fontSize: 12, marginTop: 10 }}>{L.cliNote}</div>
    </section>
  );
}
