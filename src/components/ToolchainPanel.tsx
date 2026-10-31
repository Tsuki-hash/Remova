import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { t, formatSize } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { toast } from "../lib/toast";
import { CloseGlyph } from "./ui/Glyph";
import { ScanCompletenessNotice } from "./ScanCompletenessNotice";
import type { ScanCompleteness } from "../types";

/**
 * M4E.6 (read-only): Visual Studio family audit — every instance (not just
 * -latest), workload probes from the filesystem, sizes, and a bounded
 * project-consumer search under user-chosen roots. Nothing here updates or
 * uninstalls; the functional build probe stays out of this panel.
 */
export function ToolchainPanel({ onClose }: { onClose: () => void }) {
  const L = t();
  const [result, setResult] = useState<import("../types").ToolchainAuditResult | null>(null);
  const [completeness, setCompleteness] = useState<ScanCompleteness>({
    status: "unknown",
    roots_scanned: 0,
    failed_roots: ["not_scanned"],
  });
  const [loadError, setLoadError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [rootsText, setRootsText] = useState("");
  const [consumers, setConsumers] = useState<import("../types").ConsumerSearchResult | null>(null);
  const [consumerBusy, setConsumerBusy] = useState(false);

  const scan = async () => {
    setBusy(true);
    setLoadError(null);
    try {
      const r = await api.auditToolchains();
      setResult(r);
      setCompleteness(r.completeness ?? { status: "unknown", roots_scanned: 0, failed_roots: ["legacy_payload"] });
    } catch (e) {
      setLoadError(formatError(e));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    void scan();
    // Mount-only: the load starts once; the reload button re-runs it.
  }, []);

  const searchConsumers = async () => {
    const roots = rootsText
      .split("\n")
      .map((r) => r.trim())
      .filter(Boolean);
    if (!roots.length || consumerBusy) return;
    setConsumerBusy(true);
    try {
      setConsumers(await api.findProjectConsumers(roots));
    } catch (e) {
      toast.error(formatError(e));
    } finally {
      setConsumerBusy(false);
    }
  };

  return (
    <section style={{ ...css.card, marginBottom: 16, padding: 16, fontSize: 13 }} aria-label={L.toolchainTitle}>
      <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap", marginBottom: 8 }}>
        <strong>{L.toolchainTitle}</strong>
        <span style={css.muted}>{L.toolchainHint}</span>
        <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
          <button style={{ ...css.btnGhost, height: 28 }} disabled={busy} onClick={() => void scan()}>
            {busy ? L.loadingGeneric : L.manageReload}
          </button>
          <button style={{ ...css.btnGhost, height: 28 }} onClick={onClose} aria-label={L.panelClose}>
            <CloseGlyph />
          </button>
        </div>
      </div>
      <ScanCompletenessNotice completeness={completeness} />
      {loadError && <div role="alert" style={{ color: "var(--danger-text)", margin: "6px 0" }}>{loadError}</div>}
      {result && !result.vswhere_available && (
        <div style={{ ...css.muted, fontSize: 12, margin: "6px 0" }}>{L.toolchainVswhereMissing}</div>
      )}
      {result && result.instances.length === 0 && result.vswhere_available && (
        <p style={css.muted}>{L.toolchainNoInstances}</p>
      )}
      {result &&
        result.instances.map((inst) => (
          <div key={inst.installation_path} style={{ borderTop: "1px solid var(--border)", padding: "6px 0" }}>
            <div style={{ fontWeight: 600 }}>
              {inst.display_name} <span style={{ ...css.mono, ...css.muted, fontSize: 11 }}>{inst.installation_version}</span>
            </div>
            <div className="ell" style={{ ...css.muted, fontSize: 11.5 }} title={inst.installation_path}>
              {inst.installation_path} · {formatSize(inst.size_kb)}
              {inst.size_capped ? " ≥" : ""}
            </div>
            <div style={{ display: "flex", gap: 6, flexWrap: "wrap", marginTop: 4 }}>
              <span style={{ ...css.muted, fontSize: 11 }}>
                {L.toolchainWorkload(L.toolchainMsvc, inst.has_msvc)}
              </span>
              <span style={{ ...css.muted, fontSize: 11 }}>
                {L.toolchainWorkload(L.toolchainMsbuild, inst.has_msbuild)}
              </span>
              <span style={{ ...css.muted, fontSize: 11 }}>
                {L.toolchainWorkload(L.toolchainIde, inst.has_ide)}
              </span>
            </div>
          </div>
        ))}

      <div style={{ borderTop: "1px solid var(--border)", marginTop: 10, paddingTop: 8 }}>
        <div style={{ fontWeight: 600, marginBottom: 4 }}>{L.toolchainConsumers}</div>
        <div style={{ ...css.muted, fontSize: 12, marginBottom: 6 }}>{L.toolchainConsumersHint}</div>
        <textarea
          style={{ ...css.input, width: "100%", minHeight: 56, fontFamily: "var(--mono)", fontSize: 11.5 }}
          placeholder={"D:\\projects\nC:\\code"}
          value={rootsText}
          onChange={(e) => setRootsText(e.target.value)}
          aria-label={L.toolchainConsumersRoots}
        />
        <button
          type="button"
          style={{ ...css.btnGhost, height: 28, marginTop: 4 }}
          disabled={consumerBusy || !rootsText.trim()}
          onClick={() => void searchConsumers()}
        >
          {consumerBusy ? L.loadingGeneric : L.toolchainSearchConsumers}
        </button>
        {consumers && (
          <div style={{ marginTop: 6 }}>
            <div style={{ ...css.muted, fontSize: 12 }}>
              {L.toolchainConsumerFound(consumers.hits.length)}
              {consumers.omitted > 0 ? ` · ${L.scanOmitted(consumers.omitted)}` : ""}
            </div>
            <div style={{ maxHeight: 160, overflow: "auto", border: "1px solid var(--border)", borderRadius: 8 }}>
              {consumers.hits.map((h) => (
                <div key={h.path} className="ell" style={{ ...css.mono, fontSize: 11, padding: "3px 8px" }} title={h.path}>
                  {h.path}
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
      <div style={{ ...css.muted, fontSize: 12, marginTop: 10 }}>{L.toolchainNote}</div>
    </section>
  );
}
