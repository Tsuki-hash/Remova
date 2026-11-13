import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../lib/api";
import { t, formatSize } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { cleanupAbortText, cleanupFeedback } from "../lib/cleanupFeedback";
import { requestConfirmEx } from "../lib/confirm";
import { toast } from "../lib/toast";
import { OrphanOriginGroups } from "./OrphanOriginGroups";
import { ToolGlyph } from "./ToolIcons";
import { ScanProgressBar } from "./ScanProgressBar";
import { useAssociationScan, scanCancelled } from "../hooks/useAssociationScan";
import { useRequestSeq } from "../hooks/useRequestSeq";
import {
  groupByOrigin,
  summarizeLeftovers,
  defaultSelectable,
  bucketItem,
  maxRiskOf,
  riskTierLabel,
  buildCleanupRiskBits,
  formatRiskNote,
} from "../lib/decision";
import type { CleanupItem, FullCleanupReport, InstalledApp } from "../types";
import { ScanCompletenessNotice } from "./ScanCompletenessNotice";
import { loadRestorePointPref } from "../lib/restorePointPref";
import { compareConvergence, type ConvergenceRound } from "../lib/residueConvergence";
import { summarizeCleanupPlan } from "../lib/cleanupPlan";
import { normalizeOrphanScan } from "../lib/scopedScan";
import "./OrphanPage.css";

const ORPHAN_CHANNEL = "orphan-scan";

/** Scan and review leftovers without an installed owner. */
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
  const [cleanupAllowed, setCleanupAllowed] = useState(false);
  const [lastScanCount, setLastScanCount] = useState<number | null>(null);
  const [omitted, setOmitted] = useState(0);
  const [completeness, setCompleteness] = useState<import("../types").ScanCompleteness>({
    status: "unknown",
    roots_scanned: 0,
    failed_roots: ["not_scanned"],
  });
  const [extended, setExtended] = useState(false);
  const roundsRef = useRef<ConvergenceRound[]>([]);
  const [convergenceRounds, setConvergenceRounds] = useState(0);
  const [converged, setConverged] = useState<boolean | null>(null);
  const { runReadOnly, cancel, progress } = useAssociationScan();
  const { next: nextScanRound, isCurrent: isCurrentScanRound, invalidate: invalidateScanRound } = useRequestSeq();
  const groups = useMemo(() => (items ? groupByOrigin(items) : []), [items]);
  const summary = useMemo(() => summarizeLeftovers(items || []), [items]);

  const selectedMeta = useMemo(() => {
    if (!items) return "";
    const plan = summarizeCleanupPlan(items, selected);
    const kb = plan.knownKb > 0 ? formatSize(plan.knownKb) : "";
    const unknown = plan.unknown > 0 ? ` · ${L.planUnknownSize(plan.unknown)}` : "";
    return `${L.orphanSelectedMeta(plan.count, kb)}${unknown}`;
  }, [items, selected, L]);

  const scan = async (quiet = false) => {
    if (busy) return;
    const round = nextScanRound();
    setBusy(true);
    setItems(null);
    setSelected(new Set());
    setCleanupAllowed(false);
    setLastScanCount(null);
    setOmitted(0);
    setCompleteness({ status: "unknown", roots_scanned: 0, failed_roots: ["not_scanned"] });
 // One channel for the whole run — no sticky pile-up.
    if (!quiet) toast.info(L.orphanScanProgress, { channel: ORPHAN_CHANNEL });
    try {
      const raw = await runReadOnly(
        (id: number) => api.orphanScan(id, extended),
        (r) => normalizeOrphanScan(r).items.length,
      );
      if (!isCurrentScanRound(round)) return;
      const { items: list, omitted: leftOut, completeness, cleanupAllowed } = normalizeOrphanScan(raw);
      setCompleteness(completeness);
      setCleanupAllowed(cleanupAllowed);
      // Convergence rounds: only complete scans feed the comparator; an
      // incomplete scan cannot prove the next sweep would not surface more.
      const complete = completeness.status === "complete";
      roundsRef.current = [...roundsRef.current, { items: list, complete }].slice(-2);
      const conv = compareConvergence(roundsRef.current);
      setConvergenceRounds(conv.rounds);
      setConverged(conv.converged);
      setOmitted(leftOut);
      setItems(list);
      if (cleanupAllowed) setSelected(new Set(list.filter(defaultSelectable).map((it) => it.path)));
      setLastScanCount(list.length);
      if (!quiet && complete) {
        if (list.length === 0) {
          toast.info(L.orphanScanEmpty, { channel: ORPHAN_CHANNEL });
        } else {
          const s = summarizeLeftovers(list);
          toast.success(L.orphanScanDone(s.total, s.suggest, s.keep), {
            channel: ORPHAN_CHANNEL,
            ttl: 3000,
          });
        }
      }
    } catch (e) {
      if (!isCurrentScanRound(round)) return;
      if (scanCancelled(e)) {
        if (!quiet) toast.info(L.scanCancelledIncomplete, { channel: ORPHAN_CHANNEL });
        return;
      }
      const msg = formatError(e, "analyze");
      onError?.(msg);
      toast.error(msg, { channel: quiet ? `${ORPHAN_CHANNEL}-refresh` : ORPHAN_CHANNEL });
    } finally {
      if (isCurrentScanRound(round)) setBusy(false);
    }
  };

 // Start immediately when the page opens so the click always shows work happening.
  const startedRef = useRef(false);
  useEffect(() => {
    if (startedRef.current) return;
    startedRef.current = true;
    void scan();
    // Unmounting retires every in-flight round so a late scan cannot publish.
    return () => { startedRef.current = false; invalidateScanRound(); };
 // Mount-only: the scan starts once; a later prop change must not restart it.
 // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const cleanSelected = async () => {
    if (!cleanupAllowed || !items || selected.size === 0 || busy) return;
    const picked = items.filter((it) => selected.has(it.path));
    const riskNote = formatRiskNote(buildCleanupRiskBits(picked, L), L.riskNoteTitle);
    const { ok, checked } = await requestConfirmEx({
      title: L.cleanup,
      // the item count appears once — in `cleanupConfirmOptionalBackup`.
      message: `${L.riskTierPrefix(riskTierLabel(maxRiskOf(picked), L))}\n${L.cleanupConfirmOptionalBackup(picked.length, false)}${riskNote}`,
      confirmLabel: L.cleanup,
      danger: true,
      checkbox: { label: L.confirmBackupBeforeCleanup, defaultChecked: false, status: L.cleanupBackupStatus },
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
        // the user can decline a restore point; the backend still
        // owns the decision when this is absent.
        restore_point: loadRestorePointPref(),
        cleanup_source: "orphan",
      });
      if (report.aborted) {
        const msg = cleanupAbortText(report, L);
        onError?.(msg);
        toast.error(msg, { channel: ORPHAN_CHANNEL });
        onLastReport(report);
        return;
      }
      onLastReport(report);
      cleanupFeedback(report, L, ORPHAN_CHANNEL);
      await scan(true);
    } catch (e) {
      const msg = formatError(e, "cleanup");
      onError?.(msg);
      toast.error(msg, { channel: ORPHAN_CHANNEL });
    } finally {
      setBusy(false);
    }
  };

  const selectAll = () => {
    if (!cleanupAllowed || !items || busy) return;
    const paths = items.filter((it) => bucketItem(it) !== "keep").map((it) => it.path);
    setSelected((prev) => {
      const n = new Set(prev);
      for (const p of paths) n.add(p);
      return n;
    });
  };

  const toggleMany = (paths: string[], select: boolean) => {
    setSelected((prev) => {
      const n = new Set(prev);
      for (const p of paths) {
        if (select) n.add(p);
        else n.delete(p);
      }
      return n;
    });
  };

  return (
    <div className="remova-orphan">
      <header className="remova-orphan-scan">
        <div className="remova-orphan-toolbar">
          <div className="remova-orphan-intro">
            <p>{L.orphanPageHint}</p>
            {!busy && lastScanCount !== null && !(lastScanCount === 0 && completeness.status === "complete") && (
              <span>{`${L.orphanJustScanned} · ${lastScanCount}`}</span>
            )}
          </div>
          <label className="remova-orphan-scope">
            <input type="checkbox" checked={extended} disabled={busy} onChange={(e) => { setExtended(e.target.checked); roundsRef.current = []; setConvergenceRounds(0); setConverged(null); }} />
            {L.orphanExtendedScope}
          </label>
          <button
            className="remova-orphan-scan-button"
            style={{ ...css.btnGhost, height: 36 }}
            disabled={busy}
            onClick={() => void scan()}
          >
            <ToolGlyph name="monitor" />{L.orphanScan}
          </button>
        </div>
        <ScanProgressBar progress={progress} onCancel={cancel} />
        {items && <ScanCompletenessNotice completeness={completeness} />}
        {items && !cleanupAllowed && (
          <div style={css.muted} role="note">{L.orphanReadOnly}</div>
        )}
        {convergenceRounds >= 2 && converged === true && (
          <div style={{ ...css.muted, fontSize: 12 }} role="status">
            {L.orphanConverged}
          </div>
        )}
        {convergenceRounds >= 2 && converged === false && (
          <div style={{ ...css.muted, fontSize: 12 }} role="status">
            {L.orphanNotConverged}
          </div>
        )}
        {omitted > 0 && (
          <div style={{ ...css.muted, marginTop: 6, fontSize: 12 }} role="status">
            {/* regression fix: the cap is applied before the shape probe, so the dropped
                set is candidates, not confirmed rows — say so. */}
            {L.orphanOmitted(omitted)}
          </div>
        )}
      </header>
      {items && (
        <section className="remova-orphan-results" aria-label={L.orphanResults}>
          {items.length > 0 && (
            <div className="remova-orphan-summary">
              {([
                ["safe", L.bucketSafe, summary.safe, L.bucketSafeHint],
                ["suggest", L.bucketSuggest, summary.suggest, L.bucketSuggestHint],
                ["keep", L.orphanSkip, summary.keep, L.orphanSkipHint],
              ] as const).map(([bucket, label, count, hint]) => (
                <div key={bucket} data-bucket={bucket} title={hint}>
                  <span>{label}</span> <strong>{count}</strong>
                  {bucket === "keep" && <small className="remova-orphan-summary-hint">{hint}</small>}
                </div>
              ))}
            </div>
          )}
          <div className="remova-orphan-scroll">
            {items.length === 0 ? (
              <div className="remova-orphan-empty">
                <ToolGlyph name="folder" size={30} />
                <p>{completeness.status === "complete" ? L.orphanScanEmpty : L.scanEmptyNone}</p>
              </div>
            ) : (
              <OrphanOriginGroups
                groups={groups}
                compact
                selectedPaths={selected}
                onToggle={cleanupAllowed && !busy ? (path) =>
                  setSelected((s) => {
                    const n = new Set(s);
                    if (n.has(path)) n.delete(path);
                    else n.add(path);
                    return n;
                  }) : undefined}
                onToggleMany={cleanupAllowed && !busy ? toggleMany : undefined}
              />
            )}
          </div>
          {items.length > 0 && cleanupAllowed && (
            <div className="remova-orphan-selection">
              <button style={{ ...css.btnGhost, height: 28 }} onClick={selectAll}
                title={L.orphanSelectAllHint} disabled={busy || !items.some(it => bucketItem(it) !== "keep")}>
                {L.orphanSelectAll}
              </button>
              <button
                style={{ ...css.btnGhost, height: 28 }}
                onClick={() => setSelected(new Set())}
                disabled={busy || selected.size === 0}
              >
                {L.clearSelection}
              </button>
              <span className="remova-orphan-selection-meta" role="status">{selectedMeta}</span>
              <button
                style={{
                  ...css.btn,
                  height: 32,
                  background: selected.size ? "var(--danger-text)" : undefined,
                  color: selected.size ? "var(--accent-ink)" : undefined,
                }}
                disabled={busy || selected.size === 0}
                onClick={() => void cleanSelected()}
              >
                {`${L.cleanup}${selected.size ? ` (${selected.size})` : ""}`}
              </button>
            </div>
          )}
        </section>
      )}
    </div>
  );
}
