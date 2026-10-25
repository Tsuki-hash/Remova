import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { CloseGlyph } from "./ui/Glyph";
import type { PortableScanResult, TraceScanResult } from "../types";

/**
 * Read-only system-trace and portable-candidate report. Both scans are
 * evaluation-gated: only the classes whose structure admits precise matching
 * are listed, and nothing here offers deletion.
 */
export function TracesPanel({ onClose }: { onClose: () => void }) {
  const L = t();
  const [traces, setTraces] = useState<TraceScanResult | null>(null);
  const [portable, setPortable] = useState<PortableScanResult | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const [tr, pb] = await Promise.all([
          api.consentOrphans(),
          api.portableCandidates(),
        ]);
        if (!alive) return;
        setTraces(tr);
        setPortable(pb);
      } catch (e) {
        if (alive) setLoadError(formatError(e));
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  return (
    <section style={{ ...css.card, marginBottom: 16, padding: 16, fontSize: 13 }} aria-label={L.tracesTitle}>
      <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap", marginBottom: 8 }}>
        <strong>{L.tracesTitle}</strong>
        <span style={css.muted}>{L.tracesHint}</span>
        <button style={{ ...css.btnGhost, height: 28, marginLeft: "auto" }} onClick={onClose} aria-label={L.panelClose}>
          <CloseGlyph />
        </button>
      </div>

      {loadError && (
        <div role="alert" style={{ color: "var(--danger-text)", marginBottom: 8 }}>
          {loadError}
        </div>
      )}

      <div style={{ fontWeight: 600 }}>{L.tracesConsent}</div>
      <div style={{ ...css.muted, fontSize: 12, margin: "4px 0 6px" }}>{L.tracesConsentHint}</div>
      {traces && traces.consent_orphans.length === 0 && (
        <p style={css.muted}>{L.tracesConsentEmpty}</p>
      )}
      {traces && traces.consent_orphans.length > 0 && (
        <div style={{ border: "1px solid var(--border)", borderRadius: 8, maxHeight: 180, overflow: "auto" }}>
          {traces.consent_orphans.map((o) => (
            <div
              key={`${o.key}|${o.exe_path}`}
              title={o.exe_path}
              style={{
                display: "flex",
                gap: 8,
                padding: "3px 8px",
                borderBottom: "1px solid var(--border)",
                fontSize: 12,
              }}
            >
              <span className="ell" style={{ flex: 1, minWidth: 0 }}>{o.exe_path}</span>
              <span style={{ ...css.muted, whiteSpace: "nowrap" }}>{o.capability}</span>
            </div>
          ))}
        </div>
      )}
      {traces && traces.omitted > 0 && (
        <div style={{ ...css.muted, fontSize: 12, marginTop: 4 }}>{L.scanOmitted(traces.omitted)}</div>
      )}

      <div style={{ fontWeight: 600, marginTop: 12 }}>{L.tracesPortable}</div>
      <div style={{ ...css.muted, fontSize: 12, margin: "4px 0 6px" }}>{L.tracesPortableHint}</div>
      {portable && portable.candidates.length === 0 && (
        <p style={css.muted}>{L.tracesPortableEmpty}</p>
      )}
      {portable && portable.candidates.length > 0 && (
        <div style={{ border: "1px solid var(--border)", borderRadius: 8, maxHeight: 180, overflow: "auto" }}>
          {portable.candidates.map((c) => (
            <div
              key={c.exe_path}
              title={c.exe_path}
              style={{
                display: "flex",
                gap: 8,
                padding: "3px 8px",
                borderBottom: "1px solid var(--border)",
                fontSize: 12,
              }}
            >
              <span className="ell" style={{ flex: 1, minWidth: 0 }}>{c.exe_path}</span>
              <span style={{ ...css.muted, whiteSpace: "nowrap" }}>{c.rule_name}</span>
            </div>
          ))}
        </div>
      )}
      {portable && portable.omitted > 0 && (
        <div style={{ ...css.muted, fontSize: 12, marginTop: 4 }}>{L.scanOmitted(portable.omitted)}</div>
      )}

      <div style={{ ...css.muted, fontSize: 12, marginTop: 10 }}>{L.tracesNote}</div>
    </section>
  );
}
