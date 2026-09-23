import { useCallback, useEffect, useMemo, useState } from "react";
import { api, type CleanupSourceId } from "../lib/api";
import { t, formatSize } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { requestConfirmEx } from "../lib/confirm";
import { toast } from "../lib/toast";
import { ToolGlyph } from "./ToolIcons";
import {
  defaultSelectable,
  maxRiskOf,
  riskTierLabel,
  summarizeLeftovers,
} from "../lib/decision";
import type { CleanupItem, FullCleanupReport, InstalledApp } from "../types";

const channelFor = (src: string) => `scoped-scan-${src}`;

/** Shared panel for installer / toolcache scoped scans (same trust model as orphans). */
export function ScopedScanPanel({
  title,
  hint,
  scan,
  cleanupSource,
  appName,
  onClose,
  onLastReport,
  onError,
}: {
  title: string;
  hint: string;
  scan: () => Promise<CleanupItem[]>;
  cleanupSource: Extract<CleanupSourceId, "installer" | "toolcache">;
  appName: string;
  onClose: () => void;
  onLastReport: (r: FullCleanupReport) => void;
  onError: (msg: string) => void;
}) {
  const L = t();
  const channel = channelFor(cleanupSource);
  const [items, setItems] = useState<CleanupItem[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const summary = useMemo(() => summarizeLeftovers(items || []), [items]);

  const runScan = useCallback(async () => {
    setBusy(true);
    toast.info(L.orphanScanProgress, { channel });
    try {
      const list = await scan();
      setItems(list);
      setSelected(new Set(list.filter(defaultSelectable).map((it) => it.path)));
      if (list.length === 0) toast.info(L.orphanScanEmpty, { channel });
      else {
        const s = summarizeLeftovers(list);
        toast.success(L.orphanScanDone(s.total, s.suggest, s.keep), { channel, ttl: 2500 });
      }
    } catch (e) {
      const msg = formatError(e, "analyze");
      onError(msg);
      toast.error(msg, { channel });
    } finally {
      setBusy(false);
    }
  }, [scan, L, onError, channel]);

  useEffect(() => {
    void runScan();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const cleanSelected = async () => {
    if (!items || selected.size === 0 || busy) return;
    const picked = items.filter((it) => selected.has(it.path));
    const { ok, checked } = await requestConfirmEx({
      title: L.cleanup,
      message: `${L.orphanCleanupRiskPrefix(riskTierLabel(maxRiskOf(picked), L), picked.length)}\n${L.cleanupConfirmOptionalBackup(picked.length, false)}`,
      confirmLabel: L.cleanup,
      danger: true,
      checkbox: { label: L.confirmBackupBeforeCleanup, defaultChecked: false },
    });
    if (!ok) return;
    setBusy(true);
    try {
      const app: InstalledApp = {
        name: appName,
        version: "",
        publisher: "",
        install_location: "",
        uninstall_string: "",
        quiet_uninstall_string: "",
        source: cleanupSource === "installer" ? "Installer" : "ToolCache",
        registry_key: "",
        estimated_size_kb: 0,
        install_date: "",
        display_icon: "",
      };
      const report = await api.fullCleanup(app, picked, {
        dry_run: false,
        skip_official_uninstall: true,
        backup_enabled: checked,
        cleanup_source: cleanupSource,
      });
      onLastReport(report);
      if (!report.aborted) {
        toast.success(L.batchDetail(report.deleted, report.failed), { channel, ttl: 2500 });
        await runScan();
      }
    } catch (e) {
      const msg = formatError(e, "cleanup");
      onError(msg);
      toast.error(msg, { channel });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div style={{ ...css.card, marginBottom: 12, padding: 12, fontSize: 13 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
        <strong>{title}</strong>
        <span style={css.muted}>{hint}</span>
        <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
          <button
            style={{ ...css.btn, height: 30, opacity: busy || selected.size === 0 ? 0.5 : 1 }}
            disabled={busy || selected.size === 0}
            onClick={() => void cleanSelected()}
          >
            {L.cleanup}
          </button>
          <button style={{ ...css.btnGhost, height: 30 }} disabled={busy} onClick={() => void runScan()}>
            {L.orphanScan}
          </button>
          <button style={{ ...css.btnGhost, height: 30 }} onClick={onClose}>
            ×
          </button>
        </div>
      </div>
      {items && (
        <div style={{ ...css.muted, marginTop: 6, fontSize: 12 }}>
          {summary.total} · {L.orphanSelectedMeta(selected.size, "")}
        </div>
      )}
      <div style={{ maxHeight: 320, overflow: "auto", marginTop: 8 }}>
        {items?.map((it) => (
          <label
            key={it.path}
            style={{
              display: "flex",
              gap: 8,
              alignItems: "flex-start",
              padding: "6px 4px",
              borderBottom: "1px solid var(--border)",
              cursor: "pointer",
            }}
          >
            <input
              type="checkbox"
              checked={selected.has(it.path)}
              onChange={(e) => {
                setSelected((prev) => {
                  const n = new Set(prev);
                  if (e.target.checked) n.add(it.path);
                  else n.delete(it.path);
                  return n;
                });
              }}
            />
            <span style={{ color: "var(--muted)", display: "inline-flex", marginTop: 2 }}>
              <ToolGlyph name={it.kind === "dir" ? "folder" : "file"} />
            </span>
            <span style={{ minWidth: 0, flex: 1 }}>
              <div className="ell" style={{ fontFamily: "var(--mono)", fontSize: 11.5 }}>
                {it.path}
              </div>
              <div style={{ ...css.muted, fontSize: 11 }}>
                {it.bucket || it.reason}
                {it.user_data || it.user_library ? ` · ${L.reasonUserLibrary || "user data"}` : ""}
                {it.size_kb ? ` · ${formatSize(it.size_kb)}` : ""}
              </div>
            </span>
            <button
              type="button"
              style={{ ...css.btnGhost, height: 26, padding: "0 8px", flexShrink: 0 }}
              onClick={(e) => {
                e.preventDefault();
                void api.openPath(it.path);
              }}
            >
              {L.openLocation}
            </button>
          </label>
        ))}
        {items && items.length === 0 && <div style={css.muted}>{L.orphanScanEmpty}</div>}
      </div>
    </div>
  );
}
