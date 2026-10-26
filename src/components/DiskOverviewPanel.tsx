import { useEffect, useState } from "react";
import { api, type DriveInfo } from "../lib/api";
import { t, formatSize } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { backendErrorText } from "../lib/backendText";
import { needsAdminRights, offerElevatedRestart } from "../lib/elevate";
import { toast } from "../lib/toast";
import { requestConfirmEx } from "../lib/confirm";
import { CloseGlyph } from "./ui/Glyph";
import { ToolGlyph } from "./ToolIcons";
import type {
  ComponentStoreAnalysis,
  JunctionScanResult,
  RegenerableScanResult,
  TriageClass,
  TriageEntry,
  TriageScanResult,
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
}: {
  onClose: () => void;
  onMigrate?: (path: string) => void;
}) {
  const L = t();
  const [tab, setTab] = useState<OverviewTab>("overview");
  const [drives, setDrives] = useState<DriveInfo[] | null>(null);
  const [junction, setJunction] = useState<JunctionScanResult | null>(null);
  const [triage, setTriage] = useState<TriageScanResult | null>(null);
  const [regen, setRegen] = useState<RegenerableScanResult | null>(null);
  const [regenSel, setRegenSel] = useState<Set<string>>(new Set());
  const [regenBusy, setRegenBusy] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [dismBusy, setDismBusy] = useState(false);
  const [dism, setDism] = useState<ComponentStoreAnalysis | null>(null);

  const reload = async () => {
    const [d, j, tr, rg] = await Promise.all([
      api.listLocalDrives(),
      api.junctionLedger(),
      api.spaceTriage(),
      api.regenerableLayouts(),
    ]);
    setDrives(d);
    setJunction(j);
    setTriage(tr);
    setRegen(rg);
  };

  useEffect(() => {
    let alive = true;
    setLoadError(null);
    void (async () => {
      try {
        if (alive) await reload();
      } catch (e) {
        if (alive) setLoadError(formatError(e));
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  const runDism = async () => {
    if (dismBusy) return;
    setDismBusy(true);
    try {
      setDism(await api.analyzeComponentStore());
    } catch (e) {
      const msg = formatError(e);
      if (needsAdminRights(msg)) {
        await offerElevatedRestart(L.diskOverviewElevateHint);
      } else {
        toast.error(msg);
      }
    } finally {
      setDismBusy(false);
    }
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

  const deletableItems = regen?.items.filter((i) => i.deletable) ?? [];
  const allSelected = deletableItems.length > 0 && deletableItems.every((i) => regenSel.has(i.path));

  // The triage list and the regenerable list described the same layouts twice.
  // Merge them: every triage row carries its delete gate; any regenerate
  // layout the triage scan did not list is appended so nothing disappears.
  const triageEntries = triage?.entries ?? [];
  const rows: CleanupRow[] = [
    ...triageEntries.map((e: TriageEntry) => ({
      path: e.path,
      cls: e.class,
      reason: e.reason,
      sizeKb: e.size_kb,
      regen:
        regen?.items.find((i) => i.path.toLowerCase() === e.path.toLowerCase()) ?? null,
    })),
    ...(regen?.items ?? [])
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
    const chosen = chosenPaths ?? [...regenSel];
    if (!chosen.length || regenBusy) return;
    const totalKb =
      regen?.items.filter((i) => chosen.includes(i.path)).reduce((s, i) => s + i.size_kb, 0) ?? 0;
    const { ok } = await requestConfirmEx({
      title: L.regenTitle,
      message: L.regenCleanConfirm(chosen.length, formatSize(totalKb)),
      confirmLabel: L.regenClean,
      danger: true,
    });
    if (!ok) return;
    setRegenBusy(true);
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
      await reload();
    } catch (e) {
      toast.error(backendErrorText(formatError(e)));
    } finally {
      setRegenBusy(false);
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
    <section style={{ ...css.card, marginBottom: 16, padding: 16, fontSize: 13 }} aria-label={L.diskOverviewTitle}>
      <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap", marginBottom: 8 }}>
        <strong>{L.diskOverviewTitle}</strong>
        <span style={css.muted}>{L.diskOverviewHint}</span>
        <button
          style={{ ...css.btnGhost, height: 28, marginLeft: "auto" }}
          onClick={onClose}
          aria-label={L.panelClose}
        >
          <CloseGlyph />
        </button>
      </div>

      {loadError && (
        <div role="alert" style={{ color: "var(--danger-text)", marginBottom: 8 }}>
          {loadError}
        </div>
      )}

      <div
        role="tablist"
        aria-label={L.diskOverviewTitle}
        style={{ display: "flex", gap: 4, borderBottom: "1px solid var(--border)", marginBottom: 10 }}
      >
        {tabs.map((item) => {
          const active = tab === item.id;
          return (
            <button
              key={item.id}
              type="button"
              role="tab"
              aria-selected={active}
              onClick={() => setTab(item.id)}
              style={{
                border: "none",
                borderBottom: active ? "2px solid var(--accent)" : "2px solid transparent",
                background: "transparent",
                color: active ? "var(--accent-text)" : "var(--muted)",
                padding: "6px 10px",
                fontSize: 12.5,
                fontWeight: active ? 650 : 500,
                cursor: "pointer",
              }}
            >
              {item.label}
            </button>
          );
        })}
      </div>

      {tab === "overview" && (
        <div role="tabpanel" aria-label={L.diskOverviewTabOverview}>
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
                  {L.diskOverviewDegraded(junction.degraded_roots.join("、"))}
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
                  {L.scanOmitted(junction.omitted)}
                </div>
              )}
            </div>
          )}

          {actionableCount > 0 && (
            <div style={{ marginTop: 12, display: "flex", justifyContent: "flex-end" }}>
              <button
                type="button"
                style={{ ...css.btn, height: 32, fontSize: 12.5 }}
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
        <div role="tabpanel" aria-label={L.diskOverviewTabCleanup}>
          <div style={{ fontWeight: 600 }}>{L.triageTitle}</div>
          <div style={{ ...css.muted, fontSize: 12, margin: "4px 0 6px" }}>{L.triageHint}</div>
          {rows.length === 0 ? (
            <p style={css.muted}>{L.diskOverviewEmpty}</p>
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
                        display: "grid",
                        gridTemplateColumns: "18px minmax(0, 1fr) 120px 72px 84px",
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
                      <span
                        className="ell"
                        style={{
                          minWidth: 0,
                          color: row.regen && !row.regen.deletable ? "var(--muted)" : "var(--fg)",
                        }}
                      >
                        {row.path}
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
                        {sizeKb !== null ? formatSize(sizeKb) : "—"}
                      </span>
                      <div
                        style={{
                          display: "flex",
                          gap: 6,
                          alignItems: "center",
                          justifyContent: "flex-end",
                        }}
                      >
                        {cleanable && (
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
              <details style={{ marginTop: 6 }}>
                <summary style={{ ...css.muted, fontSize: 12, cursor: "pointer" }}>
                  {L.diskOverviewCleanupRules}
                </summary>
                <div style={{ ...css.muted, fontSize: 12, marginTop: 4 }}>{L.regenHint}</div>
              </details>
              <div style={{ display: "flex", gap: 8, marginTop: 8, justifyContent: "flex-end" }}>
                <button
                  type="button"
                  style={{ ...css.btnGhost, height: 30 }}
                  onClick={() =>
                    setRegenSel(allSelected ? new Set() : new Set(deletableItems.map((i) => i.path)))
                  }
                >
                  {L.regenSelectAll}
                </button>
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
        <div role="tabpanel" aria-label={L.diskOverviewTabComponents}>
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
              style={{ ...css.btn, height: 32, opacity: dismBusy ? 0.6 : 1 }}
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
