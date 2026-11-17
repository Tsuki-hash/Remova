import { useCallback, useEffect, useState } from "react";
import { api } from "../lib/api";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { toast } from "../lib/toast";
import { CloseGlyph } from "./ui/Glyph";
import { LocalItemIcon } from "./AppIcon";
import type { PortableScanResult, ScanCompleteness, TraceScanResult } from "../types";
import { ScanCompletenessNotice } from "./ScanCompletenessNotice";

/**
 * Read-only system-trace and portable-candidate report. Both scans are
 * evaluation-gated: only the classes whose structure admits precise matching
 * are listed, and nothing here offers deletion. Each row can still reveal the
 * file or copy the path, so the panel is not a dead end for a new user.
 */
export function TracesPanel({ onClose }: { onClose: () => void }) {
  const L = t();
  const [traces, setTraces] = useState<TraceScanResult | null>(null);
  const [portable, setPortable] = useState<PortableScanResult | null>(null);
  const [shellAssoc, setShellAssoc] = useState<import("../types").ShellAssocScanResult | null>(null);
  const [shellFailed, setShellFailed] = useState(false);
  const [completeness, setCompleteness] = useState<ScanCompleteness>({
    status: "unknown",
    roots_scanned: 0,
    failed_roots: ["not_scanned"],
  });
  const [loadError, setLoadError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const scan = useCallback(async () => {
    setBusy(true);
    setLoadError(null);
    try {
      const [tr, pb, shell] = await Promise.all([
        api.consentOrphans(),
        api.portableCandidates(),
        // A failed shell-assoc scan is surfaced as a note, never folded into
        // the result list: hiding it would read as "nothing found".
        api.shellAssocOrphans().catch(() => null),
      ]);
      setTraces(tr);
      setPortable(pb);
      setShellAssoc(shell);
      setShellFailed(shell === null);
      setCompleteness(
        tr.completeness ?? { status: "unknown", roots_scanned: 0, failed_roots: ["legacy_payload"] },
      );
    } catch (e) {
      setLoadError(formatError(e));
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    void scan();
  }, [scan]);

  const copyPath = (path: string) => {
    if (!navigator.clipboard) return;
    void navigator.clipboard.writeText(path).then(
      () => toast.success(L.pathCopied),
      () => toast.error(L.pathCopyFailed),
    );
  };
  const reveal = (path: string) => {
    void api.openPath(path).catch((e) => toast.error(formatError(e)));
  };
  const openPrivacySettings = () => reveal("ms-settings:privacy");

  const loading = traces === null || portable === null;
  return (
    <section style={{ ...css.card, marginBottom: 16, padding: 16, fontSize: 13 }} aria-label={L.tracesTitle}>
      <div className="remova-tool-heading">
        <h2>{L.tracesTitle}</h2>
        <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
          <button style={{ ...css.btnGhost, height: 28 }} disabled={busy} onClick={() => void scan()}>
            {busy ? L.loadingGeneric : L.manageReload}
          </button>
          <button style={{ ...css.btnGhost, height: 28 }} onClick={onClose} aria-label={L.panelClose}>
            <CloseGlyph />
          </button>
        </div>
      </div>

      <p className="remova-tool-note">{L.tracesIntro}</p>

      {loadError && (
        <div role="alert" style={{ color: "var(--danger-text)", marginBottom: 8 }}>
          {loadError}
        </div>
      )}
      {loading && !loadError && <p style={css.muted}>{L.loadingGeneric}</p>}
      {!loading && <ScanCompletenessNotice completeness={completeness} />}

      {traces && (
        <>
          <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
            <div style={{ fontWeight: 600 }}>{L.tracesConsent}</div>
            {traces.consent_orphans.length > 0 && (
              <button
                type="button"
                style={{ ...css.btnGhost, height: 26, marginLeft: "auto" }}
                onClick={openPrivacySettings}
              >
                {L.tracesConsentSettings}
              </button>
            )}
          </div>
          <div style={{ ...css.muted, fontSize: 12, margin: "4px 0 6px" }}>{L.tracesConsentHint}</div>
          {traces.consent_orphans.length === 0 ? (
            <p style={css.muted}>
              {completeness.status === "complete" ? L.tracesConsentEmpty : L.scanEmptyNone}
            </p>
          ) : (
            <div style={{ border: "1px solid var(--border)", borderRadius: 8, maxHeight: 180, overflow: "auto" }}>
              {traces.consent_orphans.map((o) => (
                <div
                  key={`${o.key}|${o.exe_path}`}
                  style={{
                    display: "flex",
                    gap: 8,
                    padding: "6px 8px",
                    borderBottom: "1px solid var(--border)",
                    fontSize: 12,
                  }}
                >
                  <LocalItemIcon path={o.exe_path} />
                  <div style={{ minWidth: 0, flex: 1 }}>
                    <div className="ell" style={{ fontFamily: "var(--mono)", fontSize: 11.5 }} title={o.exe_path}>
                      {o.exe_path}
                    </div>
                    <div style={{ ...css.muted, fontSize: 11 }}>
                      {L.tracesCapabilityPrefix}
                      {o.capability}
                    </div>
                  </div>
                  <button
                    type="button"
                    style={{ ...css.btnGhost, height: 26, padding: "0 8px", flexShrink: 0, alignSelf: "flex-start" }}
                    aria-label={`${L.copyPath}: ${o.exe_path}`}
                    onClick={() => copyPath(o.exe_path)}
                  >
                    {L.copyPath}
                  </button>
                </div>
              ))}
            </div>
          )}
          {traces.omitted > 0 && (
            <div style={{ ...css.muted, fontSize: 12, marginTop: 4 }}>{L.scanOmitted(traces.omitted)}</div>
          )}
        </>
      )}

      {portable && (
        <>
          <div style={{ fontWeight: 600, marginTop: 12 }}>{L.tracesPortable}</div>
          <div style={{ ...css.muted, fontSize: 12, margin: "4px 0 6px" }}>{L.tracesPortableHint}</div>
          {portable.candidates.length === 0 ? (
            <p style={css.muted}>{L.tracesPortableEmpty}</p>
          ) : (
            <div style={{ border: "1px solid var(--border)", borderRadius: 8, maxHeight: 180, overflow: "auto" }}>
              {portable.candidates.map((c) => (
                <div
                  key={c.exe_path}
                  style={{
                    display: "flex",
                    gap: 8,
                    padding: "6px 8px",
                    borderBottom: "1px solid var(--border)",
                    fontSize: 12,
                  }}
                >
                  <LocalItemIcon path={c.exe_path} />
                  <div style={{ minWidth: 0, flex: 1 }}>
                    <div className="ell" style={{ fontFamily: "var(--mono)", fontSize: 11.5 }} title={c.exe_path}>
                      {c.exe_path}
                    </div>
                    <div style={{ ...css.muted, fontSize: 11 }}>
                      {L.tracesRulePrefix}
                      {c.rule_name}
                    </div>
                  </div>
                  <button
                    type="button"
                    style={{ ...css.btnGhost, height: 26, padding: "0 8px", flexShrink: 0, alignSelf: "flex-start" }}
                    aria-label={`${L.openLocation}: ${c.exe_path}`}
                    onClick={() => reveal(c.exe_path)}
                  >
                    {L.openLocation}
                  </button>
                  <button
                    type="button"
                    style={{ ...css.btnGhost, height: 26, padding: "0 8px", flexShrink: 0, alignSelf: "flex-start" }}
                    aria-label={`${L.copyPath}: ${c.exe_path}`}
                    onClick={() => copyPath(c.exe_path)}
                  >
                    {L.copyPath}
                  </button>
                </div>
              ))}
            </div>
          )}
          {portable.omitted > 0 && (
            <div style={{ ...css.muted, fontSize: 12, marginTop: 4 }}>{L.scanOmitted(portable.omitted)}</div>
          )}
        </>
      )}

      {shellFailed && (
        <div
          role="alert"
          style={{
            ...css.muted,
            fontSize: 12,
            marginTop: 10,
            padding: "6px 8px",
            borderLeft: "3px solid var(--warn)",
            background: "var(--surface-2)",
          }}
        >
          {L.shellAssocFailed}
        </div>
      )}
      {shellAssoc && (
        <ScanCompletenessNotice completeness={shellAssoc.completeness} />
      )}

      <details className="remova-tool-disclosure">
        <summary>{L.tracesTechSummary}</summary>
        <div style={{ ...css.muted, fontSize: 12, marginTop: 4 }}>

  {shellAssoc && shellAssoc.omitted > 0 && (
    <div style={{ ...css.muted, fontSize: 12 }}>{L.orphanOmitted(shellAssoc.omitted)}</div>
  )}
  {shellAssoc && shellAssoc.orphans.length > 0 && (
    <div role="note" style={{ marginTop: 10, borderTop: "1px solid var(--border)", paddingTop: 8 }}>
      <div style={{ fontWeight: 600 }}>{L.shellAssocTitle}</div>
      <div style={{ fontSize: 12, margin: "4px 0 6px" }}>{L.shellAssocHint}</div>
      <div style={{ border: "1px solid var(--border)", borderRadius: 8, maxHeight: 180, overflow: "auto" }}>
        {shellAssoc.orphans.map((o) => (
          <div key={o.class + "|" + o.exe_path} style={{ display: "flex", gap: 8, padding: "6px 8px", borderBottom: "1px solid var(--border)", fontSize: 12, alignItems: "center" }}>
            <LocalItemIcon path={o.exe_path} />
            <span style={{ ...css.mono, fontSize: 11, flexShrink: 0 }}>{o.class}</span>
            <span className="ell" style={{ ...css.mono, fontSize: 11, flex: 1 }} title={o.exe_path}>{o.exe_path}</span>
            <button type="button" style={{ ...css.btnGhost, height: 24, padding: "0 8px", flexShrink: 0 }} aria-label={L.copyPath + ": " + o.exe_path} onClick={() => copyPath(o.exe_path)}>
              {L.copyPath}
            </button>
          </div>
        ))}
      </div>
      <div style={{ fontSize: 11, marginTop: 4 }}>{L.shellAssocNote}</div>
    </div>
  )}

</div>
        <div style={{ ...css.muted, fontSize: 12, marginTop: 4 }}>{L.tracesNote}</div>
      </details>
    </section>
  );
}
