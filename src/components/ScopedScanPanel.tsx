import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { CloseGlyph } from "./ui/Glyph";
import { api, type CleanupSourceId } from "../lib/api";
import { t, formatSize } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { backendText } from "../lib/backendText";
import { cleanupAccomplishedSomething, cleanupFeedback } from "../lib/cleanupFeedback";
import { requestConfirmEx } from "../lib/confirm";
import { toast } from "../lib/toast";
import { ToolGlyph } from "./ToolIcons";
import { VirtualList } from "./ui/VirtualList";
import { ScanProgressBar } from "./ScanProgressBar";
import { useAssociationScan, scanCancelled } from "../hooks/useAssociationScan";
import { useRequestSeq } from "../hooks/useRequestSeq";
import {
  buildCleanupRiskBits,
  bucketItem,
  defaultSelectable,
  formatRiskNote,
  leftoverReasonLine,
  maxRiskOf,
  riskTierLabel,
  summarizeLeftovers,
} from "../lib/decision";
import { summarizeCleanupPlan, summarizeScanTotal } from "../lib/cleanupPlan";
import { degradedText } from "../lib/degradedText";
import { groupScannedItems, normalizeScopedScan } from "../lib/scopedScan";
import type { CleanupItem, FullCleanupReport, InstalledApp, ScopedScanResult } from "../types";
import { LeftoverSummaryBar } from "./LeftoverSummaryBar";
import { CleanupResultDetails } from "./CleanupResultDetails";
import { loadRestorePointPref } from "../lib/restorePointPref";

const channelFor = (src: string) => `scoped-scan-${src}`;

/** A rendered row is either a bucket header or one cleanup candidate. */
type Row =
  | { kind: "header"; key: string; bucket: string; count: number }
  | { kind: "item"; key: string; item: CleanupItem };

const rowKey = (row: Row) => row.key;

/**
 * mounts stable and announces only the settled text. Ticking 30 rows
 * used to re-announce the summary 30 times; the timer collapses the burst into
 * one announcement.
 */
function useSettledText(text: string, delay = 400) {
  const [settled, setSettled] = useState("");
  useEffect(() => {
    const id = setTimeout(() => setSettled(text), delay);
    return () => clearTimeout(id);
  }, [text, delay]);
  return settled;
}

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
  /** a failed scan must not leave the list saying "measuring" forever. */
  const [scanError, setScanError] = useState<string | null>(null);
  const [omitted, setOmitted] = useState(0);
  const [degraded, setDegraded] = useState<string[]>([]);
  const [busy, setBusy] = useState<boolean>(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  /** Kept on screen after the toast fades so failures stay actionable. */
  const [result, setResult] = useState<FullCleanupReport | null>(null);
  const [retryBusy, setRetryBusy] = useState(false);
  // the toast disappears in seconds; the header keeps the finding.
  const [lastScanCount, setLastScanCount] = useState<number | null>(null);
  const submitted = useRef<CleanupItem[]>([]);
  /** Backup choice from the last confirmation — a retry must not silently flip it. */
  const backupEnabled = useRef(false);
  const { runReadOnly, cancel, progress } = useAssociationScan();
  const { next: nextScanRound, isCurrent: isCurrentScanRound, invalidate: invalidateScanRound } = useRequestSeq();
  const summary = useMemo(() => summarizeLeftovers(items || []), [items]);
  // Nested directories are collapsed and unknown sizes counted, so the header can
  // say what will actually be freed instead of only how many rows are checked.
  const plan = useMemo(() => summarizeCleanupPlan(items || [], selected), [items, selected]);  // the same subtotal over every scan-found row. The biggest category is
  // often unchecked by default, so "12 MB selected" used to read as "found nothing".
  const found = useMemo(() => summarizeScanTotal(items || []), [items]);
  const summaryText = items
    ? [
        found.knownKb > 0 ? L.scanFoundTotal(formatSize(found.knownKb)) : "",
        L.orphanSelectedMeta(selected.size, formatSize(plan.knownKb)),
        plan.unknown > 0 ? L.planUnknownSize(plan.unknown) : "",
      ]
        .filter(Boolean)
        .join(" · ")
    : "";
  // the progress bar no longer announces its 250 ms stage updates.
  // This outer region collapses them into one announcement when a stage settles.
  const liveText =
    busy && progress && (progress.status === "running" || progress.status === "stopping")
      ? L.scanStage(progress.stage)
      : summaryText;
  const announcedSummary = useSettledText(liveText, busy ? 800 : 400);
  // bucket groups, largest measured item first inside each group. Backend
  // insertion order buried a multi-GB browser cache under KB-sized npm dirs.
  const rows = useMemo<Row[]>(() => {
    const out: Row[] = [];
    for (const group of groupScannedItems(items || [])) {
      out.push({ kind: "header", key: `bucket:${group.bucket}`, bucket: group.bucket, count: group.items.length });
      for (const item of group.items) out.push({ kind: "item", key: `${item.kind}:${item.path}`, item });
    }
    return out;
  }, [items]);

  const runScan = useCallback(async (quiet = false) => {
    const round = nextScanRound();
    setBusy(true);
    setItems(null);
    setScanError(null);
    setLastScanCount(null);
    setSelected(new Set());
    setOmitted(0);
    setDegraded([]);
    // an explicit rescan retires the previous report and its retry list.
    // The internal post-cleanup refresh must keep them on screen.
    if (!quiet) {
      setResult(null);
      submitted.current = [];
    }
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
      setLastScanCount(found.items.length);
      setSelected(new Set(found.items.filter(defaultSelectable).map((it) => it.path)));
      // a degraded or capped scan must not read as a complete one —
      // the toast was the only part a screen reader actually announced.
      const partial = found.degraded.length > 0;
      const text = partial ? L.scanCompleteIncomplete(found.items.length) : L.scanComplete(found.items.length);
      const full = found.omitted > 0 ? `${text} · ${L.scanOmitted(found.omitted)}` : text;
      if (!quiet && (found.items.length === 0 || partial || found.omitted > 0)) toast.info(full, { channel });
      else if (!quiet) {
        toast.success(full, { channel, ttl: 2500 });
      }
    } catch (e) {
      if (!isCurrentScanRound(round)) return;
      if (scanCancelled(e)) {
        setScanError(L.scanCancelledIncomplete);
        if (!quiet) toast.info(L.scanCancelledIncomplete, { channel });
        return;
      }
      const msg = formatError(e, "analyze");
      setScanError(L.scanFailedIncomplete);
      onError(msg);
      toast.error(msg, { channel: quiet ? `${channel}-refresh` : channel });
    } finally {
      if (isCurrentScanRound(round)) setBusy(false);
    }
  }, [scan, runReadOnly, L, onError, channel, nextScanRound, isCurrentScanRound,
      setBusy, setItems, setScanError, setLastScanCount, setSelected, setOmitted, setDegraded, setResult]);

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

  /**
   * Run a cleanup for the given candidates and keep the report on screen.
   *
   * returns whether the cleanup actually accomplished something. The retry
   * path used to read nothing and then pushed an unconditional success toast on
   * the same channel, and `toast.push` cancels every earlier toast on that
   * channel — so a rejection or a `failed > 0` / `aborted` report had its error
   * toast destroyed and replaced with "已重试 N 项".
   */
  const runCleanup = useCallback(async (
    picked: CleanupItem[],
    backupEnabled: boolean,
    quiet = false,
  ): Promise<boolean> => {
    if (picked.length === 0 || busy) return false;
    submitted.current = picked;
    setBusy(true);
    try {
      const report = await api.fullCleanup(buildApp(), picked, {
        dry_run: false,
        skip_official_uninstall: true,
        backup_enabled: backupEnabled,
        // the user can decline a restore point; the backend still
        // owns the decision when this is absent.
        restore_point: loadRestorePointPref(),
        cleanup_source: cleanupSource,
      });
      setResult(report);
      onLastReport(report);
      const message = cleanupFeedback(report, L, quiet ? `${channel}-refresh` : channel);
      if (report.aborted) onError(message);
      if (!report.aborted) {
        await runScan(true);
      }
      return cleanupAccomplishedSomething(report);
    } catch (e) {
      const msg = formatError(e, "cleanup");
      // the IPC never ran, so the previous report and its retry button
      // must not stay on screen as if they described this attempt.
      setResult(null);
      submitted.current = [];
      onError(msg);
      toast.error(msg, { channel });
      return false;
    } finally {
      setBusy(false);
    }
  }, [busy, buildApp, cleanupSource, L, onError, onLastReport, runScan, channel, setBusy, setResult]);

  const cleanSelected = async () => {
    if (!items || selected.size === 0 || busy) return;
    const picked = items.filter((it) => selected.has(it.path));
    const riskNote = formatRiskNote(buildCleanupRiskBits(picked, L), L.riskNoteTitle);
    // only one sentence carries the item count — `riskTierPrefix` states
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
      const done = await runCleanup(picked, backupEnabled.current, true);
      // only report a successful retry when the retry actually deleted
      // something. An error toast on this same channel was previously cancelled
      // and replaced by this success toast.
      if (done) toast.success(L.cleanupRetriedDone(picked.length), { channel, ttl: 2500 });
    } finally {
      setRetryBusy(false);
    }
  };

  /** Bucket headers use the same localized bucket names as the report view. */
  const groupLabel = (bucket: string) => {
    if (!bucket) return L.linkedOther;
    // the installer panel's two groups used to repeat the panel title.
    if (cleanupSource === "installer") {
      if (bucket === "updater_cache") return L.scanEvidenceLabel("updater_cache", "");
      if (bucket === "download_pkg") return L.scanEvidenceLabel("installer_package", "");
    }
    const text = backendText(bucket, "scan");
    return text === L.backendDetailUnavailable ? bucket : text;
  };

  /**
   * the row reason was written for an installed app. In these
   * panels "belongs to the software" or "suspected leftover of the software"
   * names an app that does not exist; say what the category is instead.
   */
  const reasonLine = (it: CleanupItem) => {
    if (it.bucket === "updater_cache") return L.reasonUpdaterCache;
    if (
      it.confidence === "confirmed" &&
      (it.bucket === "dev" || it.bucket === "game" || it.bucket === "browser")
    ) {
      return backendText(it.bucket, "scan");
    }
    return leftoverReasonLine(it, L);
  };

  const copyPath = (path: string) => {
    if (!navigator.clipboard) return;
    void navigator.clipboard.writeText(path).then(
      () => toast.success(L.pathCopied),
      () => toast.error(L.pathCopyFailed),
    );
  };

  return (
    <section style={{ ...css.card, marginBottom: 16, padding: 16, fontSize: 13 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
        <h2 style={{ margin: 0, fontSize: 14, fontWeight: 650 }}>{title}</h2>
        <span style={css.muted}>{hint}</span>
        {!busy && lastScanCount !== null && (
          <span style={css.muted}>
            {`${L.orphanJustScanned} · ${lastScanCount}`}
          </span>
        )}
        <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
          <button
            style={{
              ...css.btn,
              height: 30,
              background: "var(--danger-text)",
              color: "var(--surface)",
              opacity: busy || selected.size === 0 ? 0.5 : 1,
            }}
            disabled={busy || selected.size === 0}
            onClick={() => void cleanSelected()}
          >
            {L.cleanup} ({selected.size})
          </button>
          <button
            style={{ ...css.btnGhost, height: 30 }}
            disabled={busy || !items || items.length === 0}
            title={L.orphanSelectAllHint}
            onClick={() =>
              setSelected(
                new Set(
                  items!
                    // "select all" means the same thing here as on the
                    // orphan page — keep-class rows stay unchecked.
                    .filter((it) => bucketItem(it) !== "keep")
                    .map((it) => it.path),
                ),
              )
            }
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
      {/* a live region that exists before the scan and announces only
          the settled summary; the visible line stays silent. */}
      <div
        role="status"
        aria-live="polite"
        aria-atomic="true"
        style={{
          position: "absolute",
          width: 1,
          height: 1,
          margin: -1,
          padding: 0,
          overflow: "hidden",
          clip: "rect(0 0 0 0)",
          whiteSpace: "nowrap",
          border: 0,
        }}
      >
        {announcedSummary}
      </div>
      {items && (
        <div style={{ ...css.muted, marginTop: 6, fontSize: 12 }}>
          {found.knownKb > 0 ? `${L.scanFoundTotal(formatSize(found.knownKb))} · ` : ""}
          {L.orphanSelectedMeta(selected.size, formatSize(plan.knownKb))}
          {plan.unknown > 0 ? ` · ${L.planUnknownSize(plan.unknown)}` : ""}
        </div>
      )}
      {/* an omitted count is the only honest way to show a capped list. */}
      {omitted > 0 && (
        <div style={{ ...css.muted, marginTop: 4, fontSize: 12 }}>{L.scanOmitted(omitted)}</div>
      )}
      {/* "could not read these roots" is a different fact from "found nothing". */}
      {degraded.length > 0 && (
        <div style={{ ...css.muted, marginTop: 4, fontSize: 12 }}>
          <div>{L.scanDegradedHint}</div>
          <ul style={{ margin: "2px 0 0", paddingLeft: 18 }}>
            {degraded.map((root) => <li key={root}>{degradedText(root)}</li>)}
          </ul>
        </div>
      )}
      <LeftoverSummaryBar summary={summary} scanning={busy} />
      <ScanProgressBar progress={progress} onCancel={cancel} />
      <VirtualList
        items={rows}
        height="min(320px, 40vh)"
        estimateSize={44}
        listRole
        ariaLabel={title}
        keyOf={rowKey}
        rowRole={(row) => (row.kind === "header" ? "presentation" : "listitem")}
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
              <div className="ell" style={{ fontFamily: "var(--mono)", fontSize: 11.5 }} title={row.item.path}>
                {row.item.path}
              </div>
              <div style={{ ...css.muted, fontSize: 11 }}>
                {reasonLine(row.item)}
                {row.item.reason ? ` · ${backendText(row.item.reason, "scan")}` : ""}
                {row.item.size_kb != null
                  ? ` · ${row.item.size_capped ? "≥ " : ""}${formatSize(row.item.size_kb)}`
                  : " · —"}
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
          <button
            type="button"
            style={{ ...css.btnGhost, height: 26, padding: "0 8px", flexShrink: 0, alignSelf: "flex-start" }}
            aria-label={`${L.copyPath}: ${row.item.path}`}
            onClick={() => copyPath(row.item.path)}
          >
            {L.copyPath}
          </button>
          </div>
        )}
        empty={
          items === null
            ? <div style={css.muted}>{scanError ?? L.scanEstimatingItems}</div>
            : <div style={css.muted}>{L.scanEmptyNone}</div>
        }
      />
      {/* the toast is gone in seconds; the per-item outcome stays put. */}
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
