import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { t, formatSize } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { backendErrorText } from "../lib/backendText";
import { CloseGlyph } from "./ui/Glyph";
import { ScanCompletenessNotice } from "./ScanCompletenessNotice";
import type { MsiConsistencyReport, ProductFamily, ScanCompleteness } from "../types";

/** The source label shown per vector row (localized via colSource). */
function sourceLabel(source: string): string {
  return source;
  return source;
}

/**
 * M4E.2 (read-only): product families with one row per install vector, so a
 * product installed per-machine AND per-user (or as an MSIX / CLI twin) is
 * visible as what it is. Grouping is display-only — each row is still
 * uninstalled through its own record.
 */
export function VectorsPanel({ onClose }: { onClose: () => void }) {
  const L = t();
  const [families, setFamilies] = useState<ProductFamily[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const families = await api.installVectors();
        if (!alive) return;
        setFamilies(families);
      } catch (e) {
        if (alive) setLoadError(formatError(e));
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  return (
    <section style={{ ...css.card, marginBottom: 16, padding: 16, fontSize: 13 }} aria-label={L.vectorsTitle}>
      <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap", marginBottom: 8 }}>
        <strong>{L.vectorsTitle}</strong>
        <span style={css.muted}>{L.vectorsHint}</span>
        <button style={{ ...css.btnGhost, height: 28, marginLeft: "auto" }} onClick={onClose} aria-label={L.panelClose}>
          <CloseGlyph />
        </button>
      </div>

      {loadError && (
        <div role="alert" style={{ color: "var(--danger-text)", marginBottom: 8 }}>{loadError}</div>
      )}
      {!families && !loadError && <p style={css.muted}>{L.loadingGeneric}</p>}
      {families && families.length === 0 && <p style={css.muted}>{L.vectorsEmpty}</p>}
      {families && families.length > 0 && (
        <p style={{ ...css.muted, fontSize: 12 }}>{L.vectorsFamilies(families.length)}</p>
      )}

      {(families ?? []).map((family) => (
        <details key={family.family} style={{ marginTop: 6, borderTop: "1px solid var(--border)", paddingTop: 6 }}>
          <summary style={{ cursor: "pointer", fontWeight: 600 }}>
            {family.family}{" "}
            <span style={css.muted}>{L.vectorsVectorCount(family.vectors.length)}</span>
          </summary>
          {family.vectors.map((v) => (
            <div
              key={v.vector_id}
              style={{
                display: "flex",
                gap: 10,
                padding: "6px 8px",
                borderBottom: "1px solid var(--border)",
                fontSize: 12,
                alignItems: "center",
                flexWrap: "wrap",
              }}
            >
              <span style={{ ...css.mono, flexShrink: 0 }}>{sourceLabel(v.source)}</span>
              <span>{v.version}</span>
              <span className="ell" style={{ ...css.muted, flex: 1, minWidth: 120 }} title={v.install_location}>
                {v.install_location || L.vectorsNoLocation}
              </span>
              {v.estimated_size_kb > 0 && (
                <span style={{ ...css.mono, color: "var(--muted)" }}>{formatSize(v.estimated_size_kb)}</span>
              )}
              {v.per_user && <span style={{ ...css.muted, fontSize: 11 }}>{L.vectorsPerUser}</span>}
            </div>
          ))}
        </details>
      ))}
    </section>
  );
}

/**
 * M4E.3 (read-only): MSI database cross-check. Each row names which of the
 * three views (Uninstall record / Installer key / product database) answered,
 * and what that means — the class is the diagnosis, not a deletion offer.
 */
export function MsiPanel({ onClose }: { onClose: () => void }) {
  const L = t();
  const [report, setReport] = useState<MsiConsistencyReport | null>(null);
  const [completeness, setCompleteness] = useState<ScanCompleteness>({
    status: "unknown",
    roots_scanned: 0,
    failed_roots: ["not_scanned"],
  });
  const [loadError, setLoadError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const scan = async () => {
    setBusy(true);
    setLoadError(null);
    try {
      const r = await api.msiConsistency();
      setReport(r);
      setCompleteness(r.completeness ?? { status: "unknown", roots_scanned: 0, failed_roots: ["legacy_payload"] });
    } catch (e) {
      setLoadError(backendErrorText(formatError(e)));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    void scan();
    // Mount-only: the load starts once; the reload button re-runs it.
  }, []);

  const classLabel = (cls: string): string =>
    ({
      consistent: L.msiClassConsistent,
      missing_location: L.msiClassMissingLocation,
      record_only: L.msiClassRecordOnly,
      key_only: L.msiClassKeyOnly,
      db_missing: L.msiClassDbMissing,
    })[cls] ?? cls;

  return (
    <section style={{ ...css.card, marginBottom: 16, padding: 16, fontSize: 13 }} aria-label={L.msiTitle}>
      <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap", marginBottom: 8 }}>
        <strong>{L.msiTitle}</strong>
        <span style={css.muted}>{L.msiHint}</span>
        <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
          <button style={{ ...css.btnGhost, height: 28 }} disabled={busy} onClick={() => void scan()}>
            {busy ? L.loadingGeneric : L.manageReload}
          </button>
          <button style={{ ...css.btnGhost, height: 28 }} onClick={onClose} aria-label={L.panelClose}>
            <CloseGlyph />
          </button>
        </div>
      </div>
      {loadError && <div role="alert" style={{ color: "var(--danger-text)", marginBottom: 8 }}>{loadError}</div>}
      <ScanCompletenessNotice completeness={completeness} />
      {report && report.notes.length > 0 && (
        <div style={{ ...css.muted, fontSize: 12, margin: "6px 0" }}>
          {report.notes.map((n) => (
            <div key={n}>{n === "msi_db_ok" ? L.msiDbOk : L.msiDbUnavailable}</div>
          ))}
        </div>
      )}
      {report && report.products.length === 0 && <p style={css.muted}>{L.msiEmpty}</p>}
      {report && report.products.length > 0 && (
        <div style={{ border: "1px solid var(--border)", borderRadius: 8, maxHeight: 260, overflow: "auto" }}>
          {report.products.map((d) => (
            <div
              key={d.product_code}
              style={{ display: "flex", gap: 8, padding: "6px 8px", borderBottom: "1px solid var(--border)", fontSize: 12, alignItems: "center", flexWrap: "wrap" }}
            >
              <span style={{ flexShrink: 0, fontWeight: 600 }}>{classLabel(d.class)}</span>
              <span style={{ flex: 1, minWidth: 100 }}>{d.display_name || L.msiUnnamed}</span>
              <span style={{ ...css.mono, ...css.muted, fontSize: 11 }}>{d.product_code}</span>
            </div>
          ))}
        </div>
      )}
      <div style={{ ...css.muted, fontSize: 12, marginTop: 10 }}>{L.msiNote}</div>
    </section>
  );
}
