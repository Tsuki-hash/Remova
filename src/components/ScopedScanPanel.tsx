import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { CloseGlyph } from "./ui/Glyph";
import { api, type CleanupSourceId } from "../lib/api";
import { t, formatSize } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { backendText } from "../lib/backendText";
import { cleanupFeedback } from "../lib/cleanupFeedback";
import { requestConfirmEx } from "../lib/confirm";
import { toast } from "../lib/toast";
import { ToolGlyph } from "./ToolIcons";
import { VirtualList } from "./ui/VirtualList";
import { ScanProgressBar } from "./ScanProgressBar";
import { useAssociationScan, scanCancelled } from "../hooks/useAssociationScan";
import { useRequestSeq } from "../hooks/useRequestSeq";
import {
  buildCleanupRiskBits,
  defaultSelectable,
  formatRiskNote,
  leftoverReasonLine,
  maxRiskOf,
  riskTierLabel,
  summarizeLeftovers,
} from "../lib/decision";
import { summarizeCleanupPlan } from "../lib/cleanupPlan";
import { groupScannedItems, normalizeScopedScan } from "../lib/scopedScan";
import type { CleanupItem, FullCleanupReport, InstalledApp, ScopedScanResult } from "../types";
import { LeftoverSummaryBar } from "./LeftoverSummaryBar";
import { CleanupResultDetails } from "./CleanupResultDetails";

const channelFor = (src: string) => `scoped-scan-${src}`;

/** A rendered row is either a bucket header or one cleanup candidate. */
type Row =
  | { kind: "header"; key: string; bucket: string; count: number }
  | { kind: "item"; key: string; item: CleanupItem };

const rowKey = (row: Row) => row.key;

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
  scan: (scanId?: number) => Promise<ScopedScanResult>;
  cleanupSource: Extract<CleanupSourceId, "installer" | "toolcache">;
  appName: string;
  onClose: () => void;
  onLastReport: (r: FullCleanupReport) => void;
  onError: (msg: string) => void;
}) {
  const L = t();
  const channel = channelFor(cleanupSource);
  const [items, setItems] = useState<CleanupItem[] | null>(null);
  const [omitted, setOmitted] = useState(0);
  const [degraded, setDegraded] = useState<string[]>([]);
  const [busy, setBusy] = useState<boolean>(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  /** Kept on screen after the toast fades so failures stay actionable (UX-04). */
  const [result, setResult] = useState<FullCleanupReport | null>(null);
  const [retryBusy, setRetryBusy] = useState(false);
  const submitted = useRef<CleanupItem[]>([]);
  /** Backup choice from the last confirmation — a retry must not silently flip it. */
  const backupEnabled = useRef(false);
  const { runReadOnly, cancel, progress } = useAssociationScan();
  const { next: nextScanRound, isCurrent: isCurrentScanRound, invalidate: invalidateScanRound } = useRequestSeq();
  const summary = useMemo(() => summarizeLeftovers(items || []), [items]);
  // Nested directories are collapsed and unknown sizes counted, so the header can
  // say what will actually be freed instead of only how many rows are checked.
  const plan = useMemo(() => summarizeCleanupPlan(items || [], selected), [items, selected]);
  // UX-05: bucket groups, largest measured item first inside each group. Backend
  // insertion order buried a multi-GB browser cache under KB-sized npm dirs.
  const rows = useMemo<Row[]>(() => {
    const out: Row[] = [];
    for (const group of groupScannedItems(items || [])) {
      out.push({ kind: "header", key: `bucket:${group.bucket}`, bucket: group.bucket, count: group.items.length });
      for (const item of group.items) out.push({ kind: "item", key: item.path, item });
    }
    return out;
  }, [items]);

  const runScan = useCallback(async (quiet = false) => {
    const round = nextScanRound();
    setBusy(true);
    setItems(null);
    setSelected(new Set());
    setOmitted(0);
    setDegraded([]);
    if (!quiet) toast.info(L.scanInProgress, { channel });
    try {
      // `normalizeScopedScan` runs before the progress counter reads the result,
      // so a bare-array payload (backend rollout in flight) still reports a count.
      const found = await runReadOnly(
        (id: number) => scan(id).then(normalizeScopedScan),
        (r: ScopedScanResult) => r.items.length,
      );
      if (!isCurrentScanRound(round)) return;
      setItems(found.items);
      setOmitted(found.omitted);
      setDegraded(found.degraded);
      setSelected(new Set(found.items.filter(defaultSelectable).map((it) => it.path)));
      if (!quiet && found.items.length === 0) toast.info(L.scanComplete(0), { channel });
      else if (!quiet) {
        toast.success(L.scanComplete(found.items.length), { channel, ttl: 2500 });
      }
    } catch (e) {
      if (!isCurrentScanRound(round)) return;
      if (scanCancelled(e)) {
        if (!quiet) toast.info(L.scanCancelledIncomplete, { channel });
        return;
      }
      const msg = formatError(e, "analyze");
      onError(msg);
      toast.error(msg, { channel: quiet ? `${channel}-refresh` : channel });
    } finally {
      if (isCurrentScanRound(round)) setBusy(false);
    }
  }, [scan, runReadOnly, L, onError, channel, nextScanRound, isCurrentScanRound]);

  useEffect(() => {
    void runScan();
    // Unmounting retires every in-flight round so a late scan cannot publish.
    return invalidateScanRound;
 // Mount-only: the scan starts once; a later prop change must not restart it.
 // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const buildApp = useCallback((): InstalledApp => ({
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
  }), [appName, cleanupSource]);

  /** Run a cleanup for the given candidates and keep the report on screen. */
  const runCleanup = useCallback(async (picked: CleanupItem[], backupEnabled: boolean, quiet = false) => {
    if (picked.length === 0 || busy) return;
    submitted.current = picked;
    setBusy(true);
    try {
      const report = await api.fullCleanup(buildApp(), picked, {
        dry_run: false,
        skip_official_uninstall: true,
        backup_enabled: backupEnabled,
        cleanup_source: cleanupSource,
      });
      setResult(report);
      onLastReport(report);
      const message = cleanupFeedback(report, L, quiet ? `${channel}-refresh` : channel);
      if (report.aborted) onError(message);
      if (!report.aborted) {
        await runScan(true);
      }
    } catch (e) {
      const msg = formatError(e, "cleanup");
      onError(msg);
      toast.error(msg, { channel });
    } finally {
      setBusy(false);
    }
  }, [busy, buildApp, cleanupSource, L, onError, onLastReport, runScan, channel]);

  const cleanSelected = async () => {
    if (!items || selected.size === 0 || busy) return;
    const picked = items.filter((it) => selected.has(it.path));
    const riskNote = formatRiskNote(buildCleanupRiskBits(picked, L), L.riskNoteTitle);
    // UX-08: only one sentence carries the item count — `riskTierPrefix` states
    // the risk level, `cleanupConfirmOptionalBackup` states how many items.
    const { ok, checked } = await requestConfirmEx({
      title: L.cleanup,
      message: `${L.riskTierPrefix(riskTierLabel(maxRiskOf(picked), L))}\n${L.cleanupConfirmOptionalBackup(picked.length, false)}${riskNote}`,
      confirmLabel: L.cleanup,
      danger: true,
      checkbox: { label: L.confirmBackupBeforeCleanup, defaultChecked: false, status: L.cleanupBackupStatus },
    });
    if (!ok) return;
    backupEnabled.current = checked;
    await runCleanup(picked, checked);
  };

  const retryFailed = async (paths: string[]) => {
    const byPath = new Map(submitted.current.map((it) => [it.path, it]));
    const picked = paths.map((path) => byPath.get(path)).filter((it): it is CleanupItem => !!it);
    if (picked.length === 0) {
      toast.info(L.cleanupRetryNothing, { channel });
      return;
    }
    setRetryBusy(true);
    try {
      await runCleanup(picked, backupEnabled.current, true);
      toast.success(L.cleanupRetriedDone(picked.length), { channel, ttl: 2500 });
    } finally {
      setRetryBusy(false);
    }
  };

  /** Bucket headers use the same localized bucket names as the report view. */
  const groupLabel = (bucket: string) => {
    if (!bucket) return L.linkedOther;
    const text = backendText(bucket, "scan");
    return text === L.backendDetailUnavailable ? bucket : text;
  };

  return (
    <section style={{ ...css.card, marginBottom: 16, padding: 16, fontSize: 13 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
        <h2 style={{ margin: 0, fontSize: 13.5, fontWeight: 650 }}>{title}</h2>
        <span style={css.muted}>{hint}</span>
        <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
          <button
            style={{ ...css.btn, height: 30, opacity: busy || selected.size === 0 ? 0.5 : 1 }}
            disabled={busy || selected.size === 0}
            onClick={() => void cleanSelected()}
          >
            {L.cleanup} ({selected.size})
          </button>
          <button
            style={{ ...css.btnGhost, height: 30 }}
            disabled={busy || !items || items.length === 0}
            onClick={() => setSelected(new Set(items!.map((it) => it.path)))}
          >
            {L.orphanSelectAll}
          </button>
          <button
            style={{ ...css.btnGhost, height: 30 }}
            disabled={busy || selected.size === 0}
            onClick={() => setSelected(new Set())}
          >
            {L.clearSelection}
          </button>
          <button style={{ ...css.btnGhost, height: 30 }} disabled={busy} onClick={() => void runScan()}>
            {L.scanStart}
          </button>
          <button style={{ ...css.btnGhost, height: 30 }} onClick={onClose} aria-label={L.panelClose}>
            <CloseGlyph />
          </button>
        </div>
      </div>
      {items && (
        <div style={{ ...css.muted, marginTop: 6, fontSize: 12 }} role="status">
          {L.orphanSelectedMeta(selected.size, formatSize(plan.knownKb))}
          {plan.unknown > 0 ? ` · ${L.planUnknownSize(plan.unknown)}` : ""}
        </div>
      )}
      {/* UX-05: an omitted count is the only honest way to show a capped list. */}
      {omitted > 0 && (
        <div style={{ ...css.muted, marginTop: 4, fontSize: 12 }}>{L.scanOmitted(omitted)}</div>
      )}
      {/* UX-07: "could not read these roots" is a different fact from "found nothing". */}
      {degraded.length > 0 && (
        <div style={{ ...css.muted, marginTop: 4, fontSize: 12 }}>
          <div>{L.scanDegradedHint}</div>
          <ul style={{ margin: "2px 0 0", paddingLeft: 18 }}>
            {degraded.map((root) => <li key={root}>{root}</li>)}
          </ul>
        </div>
      )}
      <LeftoverSummaryBar summary={summary} scanning={busy} />
      <ScanProgressBar progress={progress} onCancel={cancel} />
      <VirtualList
        items={rows}
        height={320}
        estimateSize={44}
        listRole
        ariaLabel={title}
        keyOf={rowKey}
        renderItem={(row) => row.kind === "header" ? (
          <div style={{ ...css.muted, fontSize: 11, fontWeight: 650, padding: "8px 4px 2px", letterSpacing: 0.3 }}>
            {groupLabel(row.bucket)} · {row.count}
          </div>
        ) : (
          <div
            style={{
              display: "flex",
              gap: 8,
              alignItems: "flex-start",
              padding: "6px 4px",
              borderBottom: "1px solid var(--border)",
            }}
          >
          <label
            style={{
              display: "flex",
              gap: 8,
              alignItems: "flex-start",
              flex: 1,
              minWidth: 0,
              cursor: "pointer",
            }}
          >
            <input
              type="checkbox"
              checked={selected.has(row.item.path)}
              onChange={(e) => {
                setSelected((prev) => {
                  const n = new Set(prev);
                  if (e.target.checked) n.add(row.item.path);
                  else n.delete(row.item.path);
                  return n;
                });
              }}
            />
            <span style={{ color: "var(--muted)", display: "inline-flex", marginTop: 2 }}>
              <ToolGlyph name={row.item.kind === "dir" ? "folder" : "file"} />
            </span>
            <span style={{ minWidth: 0, flex: 1 }}>
              <div className="ell" style={{ fontFamily: "var(--mono)", fontSize: 11.5 }}>
                {row.item.path}
              </div>
              <div style={{ ...css.muted, fontSize: 11 }}>
                {leftoverReasonLine(row.item, L)}
                {row.item.reason ? ` · ${backendText(row.item.reason, "scan")}` : ""}
                {row.item.size_kb != null ? ` · ${formatSize(row.item.size_kb)}` : " · —"}
              </div>
              <div style={{ fontSize: 10.5, marginTop: 1 }}>
                <span
                  style={{
                    color: row.item.risk === "high" ? "var(--danger-text)" : row.item.risk === "medium" ? "var(--warn-text)" : "var(--ok-text)",
                  }}
                >
                  {row.item.risk === "high" ? L.riskHigh : row.item.risk === "medium" ? L.riskMedium : L.low}
                </span>
                {" · "}
                <span style={css.muted}>
                  {row.item.confidence === "confirmed" ? L.confirmed : L.suspected}
                </span>
              </div>
            </span>
          </label>
          <button
            type="button"
            style={{ ...css.btnGhost, height: 26, padding: "0 8px", flexShrink: 0, alignSelf: "flex-start" }}
            onClick={() => void api.openPath(row.item.path).catch(e => onError(formatError(e)))}
          >
            {L.openLocation}
          </button>
          </div>
        )}
        empty={
          items === null
            ? <div style={css.muted}>{L.scanEstimatingItems}</div>
            : <div style={css.muted}>{L.scanEmptyNone}</div>
        }
      />
      {/* UX-04: the toast is gone in seconds; the per-item outcome stays put. */}
      {result && !!result.item_details.length && (
        <CleanupResultDetails
          key={JSON.stringify(result.item_details)}
          items={result.item_details}
          title={L.cleanupResultTitle}
          onRetry={result.aborted ? undefined : (paths) => retryFailed(paths)}
          retryBusy={retryBusy}
        />
      )}
    </section>
  );
}
