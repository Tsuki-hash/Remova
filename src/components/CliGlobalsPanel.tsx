import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { t, formatSize } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { backendErrorText } from "../lib/backendText";
import { requestConfirmEx } from "../lib/confirm";
import { toast } from "../lib/toast";
import { CloseGlyph } from "./ui/Glyph";
import type { CliGlobalItem, CliGlobalScanResult, ScanCompleteness } from "../types";
import { ScanCompletenessNotice } from "./ScanCompletenessNotice";

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
  const [completeness, setCompleteness] = useState<ScanCompleteness>({
    status: "unknown",
    roots_scanned: 0,
    failed_roots: ["not_scanned"],
  });
  const [sel, setSel] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);

  const reload = async () => {
    setSel(new Set());
    const next = await api.cliGlobals();
    setCompleteness(
      next.completeness ?? { status: "unknown", roots_scanned: 0, failed_roots: ["legacy_payload"] },
    );
    setResult(next);
  };

  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        await reload();
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
      toast.error(backendErrorText(formatError(e)));
    }
  };

  const toggle = (name: string) => {
    setSel((prev) => {
      const next = new Set(prev);
      if (next.has(name)) {
        next.delete(name);
      } else {
        next.add(name);
      }
      return next;
    });
  };

  const npmRows = result?.items.filter((i) => i.manager === "npm") ?? [];

  const removeSelected = async () => {
    const names = [...sel];
    if (!names.length || busy) return;
    const { ok } = await requestConfirmEx({
      title: L.cliRemoveTitle,
      message: L.cliRemoveConfirm(names.length),
      confirmLabel: L.cliRemove,
      danger: true,
    });
    if (!ok) return;
    setBusy(true);
    try {
      const report = await api.removeNpmGlobals(names.map((name) => ({ name })));
      if (report.failed.length) {
        toast.error(L.cliRemoveFailed(report.failed.length), {
          detail: report.failed.join("\n"),
        });
      }
      if (report.removed.length) {
        toast.success(L.cliRemoveDone(report.removed.length), {
          detail: `${L.restorePreviewTitle}: ${report.backup_dir.split(/[\\/]/).filter(Boolean).at(-1)}`,
        });
      }
      await reload();
    } catch (e) {
      toast.error(backendErrorText(formatError(e)));
    } finally {
      setBusy(false);
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
        <p style={css.muted}>
          {completeness.status === "complete" ? L.cliEmpty : L.scanEmptyNone}
        </p>
      )}
      <ScanCompletenessNotice completeness={completeness} />

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
                    alignItems: "center",
                  }}
                >
                  {m === "npm" && (
                    <input
                      type="checkbox"
                      aria-label={r.name}
                      checked={sel.has(r.name)}
                      onChange={() => toggle(r.name)}
                    />
                  )}
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

      {npmRows.length > 0 && (
        <div style={{ display: "flex", gap: 8, marginTop: 8, alignItems: "center" }}>
          <span style={{ ...css.muted, fontSize: 12, flex: 1 }}>{L.cliRemoveNote}</span>
          <button
            type="button"
            style={{
              ...css.btn,
              height: 30,
              flexShrink: 0,
              background: "var(--danger-text)",
              color: "var(--surface)",
              opacity: busy || sel.size === 0 ? 0.5 : 1,
            }}
            disabled={busy || sel.size === 0}
            onClick={() => void removeSelected()}
          >
            {busy ? L.cliRemoving : L.cliRemove}
          </button>
        </div>
      )}

      <div style={{ ...css.muted, fontSize: 12, marginTop: 10 }}>{L.cliNote}</div>
    </section>
  );
}
