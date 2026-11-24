import { BackButton } from "./ui/BackButton";
import { useCallback, useEffect, useState } from "react";
import { api } from "../lib/api";
import { t, formatSize } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { backendErrorText } from "../lib/backendText";
import { needsAdminRights, offerElevatedRestart } from "../lib/elevate";
import { toast } from "../lib/toast";
import { requestConfirmEx } from "../lib/confirm";
import { ToolGlyph } from "./ToolIcons";
import { LocalItemIcon } from "./AppIcon";
import { useRequestSeq } from "../hooks/useRequestSeq";
import type {
  ComponentStoreAnalysis,
  RegenerableScanResult,
  TriageClass,
  TriageEntry,
} from "../types";

const DEAD_LINKS_SHOWN = 50;

// Class names are status text, not links: only the delete/DISM classes keep a
// warning tint. Migration candidates used the accent color and read as
// clickable links even though they were static.
const TRIAGE_CLASS_TONE: Record<TriageClass, string> = {
  delete_regenerable: "var(--warn-ink)",
  via_dism: "var(--warn-ink)",
  migrate_candidate: "var(--muted)",
  leave: "var(--muted)",
  show_only: "var(--muted)",
};

/** Explicit per-class label lookup — no dynamic key indexing on the strings. */
function triageClassLabel(L: ReturnType<typeof t>, cls: TriageClass): string {
  switch (cls) {
    case "delete_regenerable":
      return L.triageClassDelete;
    case "via_dism":
      return L.triageClassViaDism;
    case "migrate_candidate":
      return L.triageClassMigrate;
    case "leave":
      return L.triageClassLeave;
    case "show_only":
      return L.triageClassShowOnly;
  }
}

function triageReasonText(L: ReturnType<typeof t>, reason: string): string {
  const map: Record<string, string> = {
    win_update_download: L.triageReasonWinUpdateDownload,
    cbs_temp: L.triageReasonCbsTemp,
    win_temp: L.triageReasonWinTemp,
    crash_dumps: L.triageReasonCrashDumps,
    winsxs_dism_only: L.triageReasonWinsxs,
    installer_source_cache: L.triageReasonInstallerCache,
    system_tree: L.triageReasonSystemTree,
    wer_reports: L.triageReasonWer,
    program_body: L.triageReasonProgramBody,
    programdata_app: L.triageReasonProgramdataApp,
    user_temp: L.triageReasonUserTemp,
    shader_cache: L.triageReasonShaderCache,
    app_data: L.triageReasonAppData,
    user_data: L.triageReasonUserData,
    system_swap_file: L.triageReasonSwapFile,
  };
  return map[reason] ?? reason;
}

function gb(n: number): string {
  return `${n.toFixed(1)} GB`;
}

/** Parent folder of a path — dead junctions can only be shown from there. */
function parentPath(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const cut = Math.max(trimmed.lastIndexOf("\\"), trimmed.lastIndexOf("/"));
  return cut > 0 ? trimmed.slice(0, cut) : trimmed;
}

type OverviewTab = "overview" | "cleanup" | "components";

// A panel can close while its Windows reads are still running. Reopening
// subscribes to the same reads, and later refreshes start fresh ones.
function sharePendingScan<T>(scan: () => Promise<T>) {
  let pending: Promise<T> | undefined;
  const run = () => {
    pending ??= Promise.resolve(scan()).finally(() => { pending = undefined; });
    return pending;
  };
  return Object.assign(run, { pending: () => pending });
}

const scanVolumes = sharePendingScan(api.listLocalDrives);
const scanJunctions = sharePendingScan(api.junctionLedger);
const scanDirectories = sharePendingScan(api.spaceTriage);
const scanCaches = sharePendingScan(api.regenerableLayouts);
const scanComponentStore = sharePendingScan(api.analyzeComponentStore);

function useDiskScan<T>(scan: (() => Promise<T>) | null) {
  const [state, setState] = useState<{ data: T | null; pending: boolean; error: string | null }>({
    data: null, pending: scan !== null, error: null,
  });
  const { next, isCurrent, invalidate } = useRequestSeq();
  const reload = useCallback(async () => {
    if (!scan) return;
    const id = next();
    setState({ data: null, pending: true, error: null });
    try {
      const data = await scan();
      if (isCurrent(id)) setState({ data, pending: false, error: null });
    } catch (error) {
      if (isCurrent(id)) setState({ data: null, pending: false, error: formatError(error) });
    }
  }, [isCurrent, next, scan]);
  useEffect(() => {
    void reload();
    return invalidate;
  }, [invalidate, reload]);
  return { ...state, reload };
}

function useRegenerableScan(path?: string) {
  const scan = useCallback(() => path === undefined ? scanCaches() : api.regenerableLayouts(path), [path]);
  return useDiskScan(scan);
}

export function RegenerableCacheHint({ path, onOpen }: {
  path: string;
  onOpen: (path: string, opener: HTMLButtonElement) => void;
}) {
  const L = t();
  const scan = useRegenerableScan(path);
  const item = scan.data?.items.find(item => item.path === path);
  if (scan.pending) return <p role="status" className="remova-tool-note">{L.diskRadarCacheChecking}</p>;
  if (scan.error) return <p className="remova-tool-note">
    <span role="status">{L.diskRadarCacheCheckFailed}</span>{" "}
    <button type="button" style={css.btnGhost} onClick={() => void scan.reload()}>{L.retry}</button>
  </p>;
  if (!item) return <p className="remova-tool-note">{L.diskRadarCacheUnavailable}</p>;
  if (!item.deletable) return <p className="remova-tool-note">{
    item.block_reason === "servicing_busy" ? L.regenBlockServicing : L.diskRadarCacheUnavailable
  }</p>;
  return <button type="button" style={css.btnGhost}
    onClick={event => onOpen(item.path, event.currentTarget)}>{L.diskRadarCacheOpen}</button>;
}

/** One row of the merged action list: triage classification + its delete gate. */
type CleanupRow = {
  path: string;
  cls: TriageClass;
  reason: string;
  sizeKb: number | null;
  regen: RegenerableScanResult["items"][number] | null;
};

/** Cleanable first, then blocked deletes, migration candidates, the rest. */
function cleanupRank(row: CleanupRow): number {
  if (row.cls === "delete_regenerable") return row.regen?.deletable ? 0 : 1;
  if (row.cls === "migrate_candidate") return 2;
  if (row.cls === "via_dism") return 3;
  return 4;
}

function cleanupSizeKb(row: CleanupRow): number {
  return row.regen?.size_kb ?? row.sizeKb ?? 0;
}

/**
 * Read-only disk-space overview: system volume shape, the junction ledger
 * (dead links called out — they also explain "why is this folder huge or
 * empty"), and an on-demand DISM component-store analysis. Cleaning lives in
 * the regenerable section; migration candidates hand off to the migration
 * tool instead of pretending to move anything here.
 */
export function DiskOverviewPanel({
  onClose,
  onMigrate,
  focusCachePath,
  onCacheBusy,
}: {
  onClose: () => void;
  onMigrate?: (path: string) => void;
  focusCachePath?: string;
  onCacheBusy?: (busy: boolean) => void;
}) {
  const L = t();
  const focused = focusCachePath !== undefined;
  const [tab, setTab] = useState<OverviewTab>(focused ? "cleanup" : "overview");
  const volumeScan = useDiskScan(focused ? null : scanVolumes);
  const junctionScan = useDiskScan(focused ? null : scanJunctions);
  const triageScan = useDiskScan(focused ? null : scanDirectories);
  const regenScan = useRegenerableScan(focusCachePath);
  const drives = volumeScan.data;
  const junction = junctionScan.data;
  const triage = triageScan.data;
  const cacheItems = regenScan.data?.items.filter(item => !focused || item.path === focusCachePath) ?? [];
  const [regenSel, setRegenSel] = useState<Set<string>>(new Set());
  const [regenBusy, setRegenBusy] = useState(false);
  const [dismBusy, setDismBusy] = useState(() => Boolean(scanComponentStore.pending()));
  const [dism, setDism] = useState<ComponentStoreAnalysis | null>(null);
  const { next: nextDism, isCurrent: isCurrentDism, invalidate: invalidateDism } = useRequestSeq();

  const cacheScan = { ...regenScan, label: L.diskOverviewStepCaches };
  const scans = focused ? [cacheScan] : [
    { ...volumeScan, label: L.diskOverviewStepVolume },
    { ...junctionScan, label: L.diskOverviewStepJunctions },
    { ...triageScan, label: L.diskOverviewStepDirectories },
    cacheScan,
  ];
  const failed = scans.filter(scan => scan.error);
  const pending = scans.filter(scan => scan.pending);
  const cleanupPending = triageScan.pending || regenScan.pending;

  const receiveDism = useCallback(async (request: Promise<ComponentStoreAnalysis>) => {
    const id = nextDism();
    try {
      const result = await request;
      if (isCurrentDism(id)) setDism(result);
    } catch (e) {
      if (!isCurrentDism(id)) return;
      const msg = formatError(e);
      const strings = t();
      if (needsAdminRights(msg)) {
        await offerElevatedRestart(strings.diskOverviewElevateHint);
      } else {
        toast.error(msg === "dism:timeout" ? strings.diskOverviewDismTimeout
          : msg === "dism:output_limit" ? strings.diskOverviewDismOutputLimit : msg);
      }
    } finally {
      if (isCurrentDism(id)) setDismBusy(false);
    }
  }, [isCurrentDism, nextDism]);

  useEffect(() => {
    if (focused) return;
    const pendingRequest = scanComponentStore.pending();
    if (pendingRequest) void receiveDism(pendingRequest);
    return invalidateDism;
  }, [focused, invalidateDism, receiveDism]);

  const runDism = () => {
    if (dismBusy) return;
    setDismBusy(true);
    void receiveDism(scanComponentStore());
  };

  const toggleRegen = (path: string) => {
    setRegenSel((prev) => {
      const next = new Set(prev);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  };

  const reveal = (path: string) => {
    void api.openPath(path).catch((e) => toast.error(formatError(e)));
  };

  const deletableItems = cacheItems.filter((i) => i.deletable);
  const allSelected = deletableItems.length > 0 && deletableItems.every((i) => regenSel.has(i.path));

  // Merge the triage rows with their cache cleanup gates, keeping cache
  // layouts even when the triage scan did not list them.
  const triageEntries = triage?.entries ?? [];
  const rows: CleanupRow[] = [
    ...triageEntries.map((e: TriageEntry) => ({
      path: e.path,
      cls: e.class,
      reason: e.reason,
      sizeKb: e.size_kb,
      regen: cacheItems.find((i) => i.path.toLowerCase() === e.path.toLowerCase()) ?? null,
    })),
    ...cacheItems
      .filter(
        (i) => !triageEntries.some((e) => e.path.toLowerCase() === i.path.toLowerCase()),
      )
      .map((i): CleanupRow => ({
        path: i.path,
        cls: "delete_regenerable",
        reason: i.reason,
        sizeKb: i.size_kb,
        regen: i,
      })),
  ];
  const migrateCount = rows.filter((r) => r.cls === "migrate_candidate").length;
  const actionableCount = deletableItems.length + migrateCount;
  // Server order is size-only; the user needs the checkable rows first.
  rows.sort(
    (a, b) => cleanupRank(a) - cleanupRank(b) || cleanupSizeKb(b) - cleanupSizeKb(a),
  );

  const cleanRegen = async (chosenPaths?: string[]) => {
    const requested = chosenPaths ?? [...regenSel];
    const selected = deletableItems.filter(item => requested.includes(item.path));
    const chosen = selected.map(item => item.path);
    if (!chosen.length || regenBusy) return;
    const totalKb = selected.reduce((sum, item) => sum + item.size_kb, 0);
    const message = L.regenCleanConfirm(chosen.length, totalKb === 0 ? "0 KB" : formatSize(totalKb));
    const { ok } = await requestConfirmEx({
      title: L.regenTitle,
      message: focused ? `${message}\n\n${chosen[0]}` : message,
      confirmLabel: L.regenClean,
      danger: true,
    });
    if (!ok) return;
    setRegenBusy(true);
    onCacheBusy?.(true);
    try {
      const report = await api.deleteRegenerableLayouts(chosen);
      if (report.skipped.length) {
        toast.error(L.regenSkipped(report.skipped.length), {
          detail: report.skipped.map((s) => `${s.path}: ${s.message}`).join("\n"),
        });
      }
      if (report.cleaned.length) {
        toast.success(L.regenDone(report.cleaned.length, formatSize(report.reclaimed_kb)));
      }
      setRegenSel(new Set());
      await Promise.all(scans.filter(scan => !scan.pending).map(scan => scan.reload()));
    } catch (e) {
      toast.error(backendErrorText(formatError(e)));
    } finally {
      setRegenBusy(false);
      onCacheBusy?.(false);
    }
  };

  const system = drives?.find((d) => d.is_system);
  const dead = junction?.entries.filter((e) => !e.reachable) ?? [];
  const tabs: { id: OverviewTab; label: string }[] = [
    { id: "overview", label: L.diskOverviewTabOverview },
    { id: "cleanup", label: L.diskOverviewTabCleanup },
    { id: "components", label: L.diskOverviewTabComponents },
  ];
  const iconBtn = {
    ...css.btnGhost,
    width: 26,
    height: 24,
    padding: 0,
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    flexShrink: 0,
  } as const;

  return (
    <section style={{ ...css.card, marginBottom: 16, padding: 16, fontSize: 13 }} aria-label={focused ? L.regenTitle : L.diskOverviewTitle}>
      {focused ? <>
        <code style={{ overflowWrap: "anywhere" }}>{focusCachePath}</code>
        <p className="remova-tool-note">{L.diskRadarCachePreview}</p>
      </> : <div className="remova-tool-heading">
        <h2>{L.diskOverviewTitle}</h2>
        <BackButton onClick={onClose} style={{ marginLeft: "auto" }} />
      </div>}

      {pending.length > 0 && (
        <div style={{ padding: "8px 10px", borderRadius: 8, background: "var(--surface-2)", marginBottom: 8 }}>
          <div style={{ display: "flex", justifyContent: "space-between", flexWrap: "wrap", gap: 6, fontWeight: 600 }}>
            <span>{L.diskOverviewProgressLabel}</span>
            <span>{L.diskOverviewProgressCount(scans.length - pending.length, scans.length)}</span>
          </div>
          <progress className="remova-scan-meter" aria-label={L.diskOverviewProgressLabel} value={scans.length - pending.length} max={scans.length} />
          <div role="status" style={{ ...css.muted, marginTop: 4 }}>{L.diskOverviewProgressPending(pending.map(scan => scan.label).join(" · "))}</div>
        </div>
      )}

      {failed.length > 0 && (
        <div style={{ marginBottom: 8, display: "flex", flexWrap: "wrap", alignItems: "center", gap: 8 }}>
          <div role="alert" style={{ color: "var(--danger-text)" }}>
            {L.diskOverviewLoadFailed} · {failed.map(scan => `${scan.label} (${scan.error})`).join(" · ")}
          </div>
          <button type="button" style={css.btnGhost} onClick={() => void Promise.all(failed.map(scan => scan.reload()))}>
            {L.retry}
          </button>
        </div>
      )}

      {!focused && <div
        role="tablist"
        aria-label={L.diskOverviewTitle}
        onKeyDown={(e) => {
          // WAI-ARIA tabs: arrow keys move between tabs, Home/End jump.
          const order: OverviewTab[] = ["overview", "cleanup", "components"];
          const at = order.indexOf(tab);
          let next: OverviewTab | null = null;
          if (e.key === "ArrowRight") next = order[(at + 1) % order.length] ?? null;
          else if (e.key === "ArrowLeft") next = order[(at + order.length - 1) % order.length] ?? null;
          else if (e.key === "Home") next = order[0] ?? null;
          else if (e.key === "End") next = order[order.length - 1] ?? null;
          if (next && next !== tab) {
            e.preventDefault();
            setTab(next);
            // WAI-ARIA tabs: the arrow keys move focus together with the
            // selection, or the reader keeps announcing the abandoned tab.
            document.getElementById(`disk-overview-tab-${next}`)?.focus();
          }
        }}
        style={{ display: "flex", gap: 4, borderBottom: "1px solid var(--border)", marginBottom: 10 }}
      >
        {tabs.map((item) => {
          const active = tab === item.id;
          return (
            <button
              key={item.id}
              id={`disk-overview-tab-${item.id}`}
              type="button"
              role="tab"
              aria-selected={active}
              aria-controls={`disk-overview-panel-${item.id}`}
              tabIndex={active ? 0 : -1}
              onClick={() => setTab(item.id)}
              style={{
                border: "none",
                borderBottom: active ? "2px solid var(--accent)" : "2px solid transparent",
                background: "transparent",
                color: active ? "var(--accent-text)" : "var(--muted)",
                padding: "6px 10px",
                fontSize: 13,
                fontWeight: active ? 650 : 500,
                cursor: "pointer",
              }}
            >
              {item.label}
            </button>
          );
        })}
      </div>}

      {tab === "overview" && (
        <div id="disk-overview-panel-overview" role="tabpanel" aria-labelledby="disk-overview-tab-overview" aria-label={L.diskOverviewTabOverview}>
          {drives && !system && <div style={css.muted}>{L.diskOverviewVolumeUnavailable}</div>}
          {system && (
            <div style={css.muted}>
              {L.diskOverviewSystemVolume(
                system.letter,
                gb(system.total_gb - system.free_gb),
                gb(system.total_gb),
                gb(system.free_gb),
              )}
            </div>
          )}

          {junction && (
            <div style={{ marginTop: 10 }}>
              <div style={{ fontWeight: 600, marginBottom: 4 }}>
                {L.diskOverviewJunctionSummary(junction.entries.length, dead.length)}
              </div>
              {junction.degraded_roots.length > 0 && (
                <div style={{ ...css.muted, fontSize: 12 }}>
                  {L.diskOverviewDegraded(junction.degraded_roots.join(", "))}
                </div>
              )}
              {dead.length > 0 && (
                <div
                  role="note"
                  style={{
                    marginTop: 6,
                    border: "1px solid var(--warn-ink)",
                    borderRadius: 8,
                    padding: "6px 10px",
                    fontSize: 12,
                  }}
                >
                  {dead.slice(0, DEAD_LINKS_SHOWN).map((e) => (
                    <div
                      key={e.path}
                      style={{ display: "flex", gap: 8, alignItems: "center" }}
                    >
                      <span
                        className="ell"
                        style={{ flex: 1, minWidth: 0, color: "var(--warn-ink)" }}
                        title={L.diskOverviewDeadLink(e.path, e.target ?? "?")}
                      >
                        {L.diskOverviewDeadLink(e.path, e.target ?? "?")}
                      </span>
                      <button
                        type="button"
                        style={iconBtn}
                        aria-label={`${L.openLocation}: ${e.path}`}
                        title={L.openLocation}
                        onClick={() => reveal(parentPath(e.path))}
                      >
                        <ToolGlyph name="folder" />
                      </button>
                    </div>
                  ))}
                  {dead.length > DEAD_LINKS_SHOWN && (
                    <div style={css.muted}>{L.diskOverviewOmitted(dead.length - DEAD_LINKS_SHOWN)}</div>
                  )}
                </div>
              )}
              {junction.omitted > 0 && (
                <div style={{ ...css.muted, fontSize: 12, marginTop: 4 }}>
                  {L.diskOverviewJunctionPartial}
                </div>
              )}
            </div>
          )}

          {actionableCount > 0 && (
            <div style={{ marginTop: 12, display: "flex", justifyContent: "flex-end" }}>
              <button
                type="button"
                style={{ ...css.btn, height: 32, fontSize: 13 }}
                onClick={() => setTab("cleanup")}
              >
                {L.diskOverviewOpenActions(actionableCount)}
              </button>
            </div>
          )}

          <div style={{ ...css.muted, fontSize: 12, marginTop: 10 }}>{L.diskOverviewScopeNote}</div>
        </div>
      )}

      {tab === "cleanup" && (
        <div id={focused ? undefined : "disk-overview-panel-cleanup"} role={focused ? undefined : "tabpanel"}
          aria-labelledby={focused ? undefined : "disk-overview-tab-cleanup"}>
          {rows.length > 0 && <p className="remova-tool-note">{focused ? L.regenHint : L.triageHint}</p>}
          {rows.length === 0 ? (
            !cleanupPending && !triageScan.error && !regenScan.error &&
              <p role={focused ? "status" : undefined} style={css.muted}>{focused ? L.diskRadarCacheUnavailable : L.diskOverviewEmpty}</p>
          ) : (
            <>
              <div
                role="list"
                style={{
                  maxHeight: 360,
                  overflowY: "auto",
                  overflowX: "hidden",
                  border: "1px solid var(--border)",
                  borderRadius: 8,
                }}
              >
                {rows.map((row) => {
                  const label = triageClassLabel(L, row.cls);
                  const tone = TRIAGE_CLASS_TONE[row.cls];
                  const cleanable =
                    row.cls === "delete_regenerable" && row.regen?.deletable
                      ? row.regen
                      : null;
                  const blocked = row.regen?.block_reason === "servicing_busy";
                  const sizeKb = row.regen?.size_kb ?? row.sizeKb;
                  return (
                    <div
                      key={row.path}
                      role="listitem"
                      title={triageReasonText(L, row.reason)}
                      style={{
                        display: focused ? "flex" : "grid",
                        flexWrap: "wrap",
                        gridTemplateColumns: "18px 18px minmax(0, 1fr) 120px 72px 84px",
                        columnGap: 8,
                        padding: "5px 8px",
                        borderBottom: "1px solid var(--border)",
                        fontSize: 12,
                        alignItems: "center",
                      }}
                    >
                      {row.regen ? (
                        <input
                          type="checkbox"
                          aria-label={row.path}
                          checked={regenSel.has(row.regen.path)}
                          disabled={!row.regen.deletable}
                          onChange={() => toggleRegen(row.regen!.path)}
                          style={{ margin: 0, justifySelf: "center" }}
                        />
                      ) : (
                        <span aria-hidden />
                      )}
                      <LocalItemIcon path={row.path} kind="dir" size={18} />
                      <span
                        className={focused ? undefined : "ell"}
                        style={{
                          minWidth: 0,
                          ...(focused && { flex: "1 1 140px", overflowWrap: "anywhere" }),
                          color: row.regen && !row.regen.deletable ? "var(--muted)" : "var(--fg)",
                        }}
                      >
                        {focused ? triageReasonText(L, row.reason) : row.path}
                        {blocked && <span style={css.muted}> — {L.regenBlockServicing}</span>}
                      </span>
                      <span style={{ color: tone, whiteSpace: "nowrap", fontSize: 11.5 }}>{label}</span>
                      <span
                        style={{
                          ...css.mono,
                          textAlign: "right",
                          whiteSpace: "nowrap",
                        }}
                      >
                        {sizeKb === null ? "—" : focused && row.regen?.size_capped
                          ? L.diskRadarCacheSizeFloor(sizeKb === 0 ? "0 KB" : formatSize(sizeKb))
                          : focused && sizeKb === 0 ? "0 KB" : formatSize(sizeKb)}
                      </span>
                      <div
                        style={{
                          display: "flex",
                          gap: 6,
                          alignItems: "center",
                          justifyContent: "flex-end",
                        }}
                      >
                        {!focused && cleanable && (
                          <button
                            type="button"
                            style={{ ...css.btnGhost, height: 24, padding: "0 8px", fontSize: 11, flexShrink: 0, color: "var(--danger-text)" }}
                            aria-label={`${L.triageActionClean}: ${row.path}`}
                            disabled={regenBusy}
                            onClick={() => void cleanRegen([cleanable.path])}
                          >
                            {L.triageActionClean}
                          </button>
                        )}
                        {row.cls === "migrate_candidate" && onMigrate && (
                          <button
                            type="button"
                            style={{ ...css.btnGhost, height: 24, padding: "0 8px", fontSize: 11, flexShrink: 0 }}
                            aria-label={`${L.triageActionMigrate}: ${row.path}`}
                            onClick={() => onMigrate(row.path)}
                          >
                            {L.triageActionMigrate}
                          </button>
                        )}
                        <button
                          type="button"
                          style={iconBtn}
                          aria-label={`${L.openLocation}: ${row.path}`}
                          title={L.openLocation}
                          onClick={() => reveal(row.path)}
                        >
                          <ToolGlyph name="folder" />
                        </button>
                      </div>
                    </div>
                  );
                })}
              </div>
              {triage && triage.omitted > 0 && (
                <div style={{ ...css.muted, fontSize: 12, marginTop: 4 }}>
                  {L.scanOmitted(triage.omitted)}
                </div>
              )}
              {!focused && <details style={{ marginTop: 6 }}>
                <summary style={{ ...css.muted, fontSize: 12, cursor: "pointer" }}>
                  {L.diskOverviewCleanupRules}
                </summary>
                <div style={{ ...css.muted, fontSize: 12, marginTop: 4 }}>{L.regenHint}</div>
              </details>}
              <div style={{ display: "flex", gap: 8, marginTop: 8, justifyContent: "flex-end" }}>
                {!focused && <button
                  type="button"
                  style={{ ...css.btnGhost, height: 30 }}
                  aria-pressed={allSelected}
                  onClick={() =>
                    setRegenSel(allSelected ? new Set() : new Set(deletableItems.map((i) => i.path)))
                  }
                >
                  {allSelected ? L.regenClearSelection : L.regenSelectAll}
                </button>}
                <button
                  type="button"
                  style={{
                    ...css.btn,
                    height: 30,
                    background: "var(--danger-text)",
                    color: "var(--surface)",
                    opacity: regenBusy || regenSel.size === 0 ? 0.5 : 1,
                  }}
                  disabled={regenBusy || regenSel.size === 0}
                  onClick={() => void cleanRegen()}
                >
                  {regenBusy ? L.regenCleaning : L.regenClean}
                </button>
              </div>
            </>
          )}
        </div>
      )}

      {tab === "components" && (
        <div id="disk-overview-panel-components" role="tabpanel" aria-labelledby="disk-overview-tab-components" aria-label={L.diskOverviewTabComponents}>
          <div style={{ fontWeight: 600 }}>{L.diskOverviewWinSxs}</div>
          <div style={{ ...css.muted, fontSize: 12, margin: "4px 0 6px" }}>{L.diskOverviewWinSxsHint}</div>
          {dism && (
            <div role="status" style={{ marginBottom: 6 }}>
              {L.diskOverviewWinSxsResult(
                dism.actual_size_bytes !== null ? gb(dism.actual_size_bytes / 1024 ** 3) : "?",
                dism.explorer_size_bytes !== null ? gb(dism.explorer_size_bytes / 1024 ** 3) : "?",
                dism.cleanup_recommended ? L.diskOverviewRecommendedYes : L.diskOverviewRecommendedNo,
              )}
            </div>
          )}
          <div style={{ display: "flex", justifyContent: "flex-end", marginTop: 6 }}>
            <button
              type="button"
              style={{ ...css.btn, height: 32, opacity: dismBusy ? 0.5 : 1 }}
              disabled={dismBusy}
              onClick={() => void runDism()}
            >
              {dismBusy ? L.diskOverviewAnalyzing : L.diskOverviewAnalyze}
            </button>
          </div>
        </div>
      )}
    </section>
  );
}
