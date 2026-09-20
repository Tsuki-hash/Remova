import { useMemo, useState } from "react";
import { api } from "../lib/api";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { requestConfirmEx } from "../lib/confirm";
import { toast } from "../lib/toast";
import { LeftoverSummaryBar } from "./LeftoverSummaryBar";
import { OrphanOriginGroups } from "./OrphanOriginGroups";
import { groupByOrigin, summarizeLeftovers, defaultSelectable } from "../lib/decision";
import type { CleanupItem, FullCleanupReport, InstalledApp } from "../types";

/** First-class orphan leftovers page (report §9 / FE-N5). */
export function OrphanPage({
  onLastReport,
  onError,
}: {
  onLastReport: (r: FullCleanupReport) => void;
  onError?: (e: string | null) => void;
}) {
  const L = t();
  const [items, setItems] = useState<CleanupItem[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const groups = useMemo(() => (items ? groupByOrigin(items) : []), [items]);
  const summary = useMemo(() => summarizeLeftovers(items || []), [items]);

  const scan = async () => {
    if (busy) return;
    setBusy(true);
    toast.info(L.orphanScanning);
    try {
      const list = await api.orphanScan();
      setItems(list);
      // FE-N5: same default-selection predicate as analyze/cleanup.
      setSelected(new Set(list.filter(defaultSelectable).map((it) => it.path)));
      if (list.length === 0) toast.info(L.orphanScanEmpty);
      else {
        const s = summarizeLeftovers(list);
        toast.success(L.orphanScanDone(s.total, s.suggest, s.keep));
      }
    } catch (e) {
      const msg = formatError(e, "analyze");
      onError?.(msg);
      toast.error(msg);
    } finally {
      setBusy(false);
    }
  };

  const cleanSelected = async () => {
    if (!items || selected.size === 0 || busy) return;
    const picked = items.filter((it) => selected.has(it.path));
    const { ok, checked } = await requestConfirmEx({
      title: L.cleanup,
      message: L.cleanupConfirmOptionalBackup(picked.length, false),
      confirmLabel: L.cleanup,
      danger: true,
      checkbox: { label: L.confirmBackupBeforeCleanup, defaultChecked: false },
    });
    if (!ok) return;
    setBusy(true);
    try {
      const app: InstalledApp = {
        name: L.orphanScan,
        version: "",
        publisher: "",
        install_location: "",
        uninstall_string: "",
        quiet_uninstall_string: "",
        source: "Orphan",
        registry_key: "",
        estimated_size_kb: 0,
        install_date: "",
        display_icon: "",
      };
      const report = await api.fullCleanup(app, picked, {
        dry_run: false,
        skip_official_uninstall: true,
        backup_enabled: checked,
        cleanup_source: "orphan",
      });
      if (report.aborted) {
        const msg = report.uninstall_message || formatError("cleanup aborted", "cleanup");
        onError?.(msg);
        toast.error(msg);
        return;
      }
      onLastReport(report);
      toast.success(L.batchDetail(report.deleted, report.failed));
      await scan();
    } catch (e) {
      const msg = formatError(e, "cleanup");
      onError?.(msg);
      toast.error(msg);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10, minHeight: 0, flex: 1 }}>
      <div style={{ ...css.card, padding: 12, fontSize: 13, flexShrink: 0 }}>
        <div style={{ display: "flex", gap: 10, alignItems: "center", flexWrap: "wrap" }}>
          <strong style={{ fontSize: 14 }}>{L.navOrphans}</strong>
          <span style={css.muted}>{L.orphanPageHint}</span>
          <button
            style={{ ...css.btn, marginLeft: "auto", height: 32 }}
            disabled={busy}
            onClick={() => void scan()}
          >
            {busy ? L.orphanScanning : L.orphanScan}
          </button>
        </div>
      </div>
      {items && (
        <div style={{ ...css.card, flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}>
          <LeftoverSummaryBar summary={summary} scanning={busy} />
          <div style={{ ...css.scroll, padding: 12 }}>
            {items.length === 0 ? (
              <div style={css.muted}>{L.orphanScanEmpty}</div>
            ) : (
              <OrphanOriginGroups
                groups={groups}
                selectedPaths={selected}
                onToggle={(path) =>
                  setSelected((s) => {
                    const n = new Set(s);
                    if (n.has(path)) n.delete(path);
                    else n.add(path);
                    return n;
                  })
                }
              />
            )}
          </div>
          {selected.size > 0 && (
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 10,
                padding: "10px 12px",
                borderTop: "1px solid var(--border)",
                background: "var(--surface-2)",
              }}
            >
              <span style={{ fontSize: 12.5, fontWeight: 600 }}>
                {L.cleanup} · {selected.size}
              </span>
              <button style={css.btnGhost} onClick={() => setSelected(new Set())}>
                {L.cancel}
              </button>
              <button
                style={{ ...css.btn, marginLeft: "auto", background: "var(--danger)", color: "#fff" }}
                disabled={busy}
                onClick={() => void cleanSelected()}
              >
                {`${L.cleanup} (${selected.size})`}
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
