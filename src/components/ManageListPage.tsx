import { useEffect, useId, useMemo, useRef, useState } from "react";
import { api } from "../lib/api";
import { useVirtualizer } from "@tanstack/react-virtual";
import { t } from "../i18n";
import { ManageItemIcon } from "./AppIcon";
import { cssStyles as css } from "../styles";
import { formatError, isAccessDeniedError } from "../lib/format";
import { requestConfirm } from "../lib/confirm";
import { toast } from "../lib/toast";
import { useManageList, type ManageTabId } from "../lib/useManageList";
import { looksMicrosoft } from "../lib/manageFilter";
import type { ManageItem } from "../types";
import { ScanProgressBar } from "./ScanProgressBar";
import { useListViewportHeight } from "../hooks/useListViewportHeight";
import { taskName, TaskWorkspace } from "./TaskWorkspace";

export type ManagePageTab = ManageTabId;

const ROW_H = 64;

/** Offer one-click elevate when an admin-gated action fails or is attempted. */
async function ensureElevatedForManage(): Promise<boolean> {
  try {
    const elevated = await api.isElevated();
    if (elevated) return true;
  } catch {
 // fall through to prompt
  }
  const L = t();
  const ok = await requestConfirm({
    title: L.elevateAskTitle,
    message: L.elevateAskBody,
    confirmLabel: L.elevateAskOk,
  });
  if (!ok) return false;
  try {
    await api.elevateRestart();
  } catch (e) {
    toast.error(formatError(e, "elevate"));
  }
  return false;
}

function startupSourceLabel(it: ManageItem, L: ReturnType<typeof t>) {
  const loc = it.location || "";
  if (loc.startsWith("PACKAGED::")) return L.manageSourceStore;
  if (loc.startsWith("SVC::")) return L.manageSourceService;
  if (loc.startsWith("FOLDER::")) return L.manageSourceFolder;
  return L.manageSourceRegistry;
}

function displayName(it: ManageItem) {
  return it.display_name || it.name;
}

function entrySource(it: ManageItem, tab: ManagePageTab, L: ReturnType<typeof t>) {
  if (tab === "services") return it.system_service ? L.manageSystemService : L.manageService;
  return startupSourceLabel(it, L);
}

function entryDescription(it: ManageItem) {
  const detail = it.detail?.trim();
  return detail && ![displayName(it), it.name, it.path, it.location]
    .some((value) => value?.trim().toLowerCase() === detail.toLowerCase()) ? detail : "";
}

function entryGlyph(it: ManageItem, tab: ManagePageTab) {
  if (it.system_service) return "system-service";
  if (tab === "services" || it.location.startsWith("SVC::")) return "services";
  if (it.location.startsWith("FOLDER::")) return "folder";
  if (it.location.startsWith("PACKAGED::")) return "installers";
  return "startup";
}

function startTypeLabel(it: ManageItem, L: ReturnType<typeof t>) {
  switch (it.start_type) {
    case "auto":
      return L.manageStartAuto;
    case "manual":
      return L.manageStartManual;
    case "disabled":
      return L.manageStartDisabledLabel;
    case "other":
      return L.manageStartOther;
    default:
      return it.enabled ? L.manageStatusEnabled : L.manageStatusDisabled;
  }
}

function enabledLabel(it: ManageItem, L: ReturnType<typeof t>) {
  return it.enabled ? L.manageStatusEnabled : L.manageStatusDisabled;
}

function runLabel(it: ManageItem, L: ReturnType<typeof t>) {
  return it.running ? L.manageStatusRunning : L.manageStatusStopped;
}

export function ManageListPage({
  tab,
  onError,
}: {
  tab: ManagePageTab;
  onError?: (msg: string) => void;
}) {
  const L = t();
  const [q, setQ] = useState("");
  const [onlyOn, setOnlyOn] = useState(true);
  const [hideMicrosoft, setHideMicrosoft] = useState(tab === "services");
  const [helpOpen, setHelpOpen] = useState(false);
  const [detailsItem, setDetailsItem] = useState<ManageItem | null>(null);
  const helpId = useId();
  const detailsTitleId = useId();
  const detailsRef = useRef<HTMLDialogElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  useListViewportHeight(scrollRef);

  const { items, busy, reload, cancel, progress } = useManageList(tab, onError);

  useEffect(() => {
    setHideMicrosoft(tab === "services");
  }, [tab]);

  const setEnabled = async (item: ManageItem, nextEnabled: boolean) => {
    const name = tab === "tasks" ? taskName(item) : displayName(item);
    if (!(await ensureElevatedForManage())) {
      return;
    }
    if (!nextEnabled) {
      const message =
        tab === "services"
          ? L.confirmDisableServiceVsStop(name)
          : tab === "tasks"
            ? L.confirmDisableTask(item.location)
            : L.confirmDisableStartup(name);
      const ok = await requestConfirm({
        title: L.manageDisable,
        message,
        confirmLabel: L.manageDisable,
        danger: tab === "services",
      });
      if (!ok) return;
    }
    try {
      if (tab === "startup") {
        await api.setStartupEnabled(item.location, nextEnabled);
      } else if (tab === "services") {
        await api.setServiceStartDisabled(item.name, !nextEnabled);
      } else {
        await api.setTaskEnabled(item.name, nextEnabled);
      }
      toast.success(nextEnabled ? L.manageEnabledDone(name) : L.manageDisabledDone(name));
      if (tab === "tasks" && document.activeElement?.closest(".remova-task-detail")) {
        searchRef.current?.focus({ preventScroll: true });
      }
      await reload();
    } catch (e) {
      const msg = formatError(e);
      onError?.(msg);
      toast.error(msg);
      if (isAccessDeniedError(e)) {
        await ensureElevatedForManage();
      }
    }
  };

  /** Service process run-state control — stop ≠ disable start type. */
  const setRunning = async (item: ManageItem, run: boolean) => {
    if (!(await ensureElevatedForManage())) {
      return;
    }
    if (run) {
      const ok = await requestConfirm({
        title: L.manageStartBtn,
        message: L.confirmStartService(displayName(item)),
        confirmLabel: L.manageStartBtn,
      });
      if (!ok) return;
    } else {
      const ok = await requestConfirm({
        title: L.manageStop,
        message: L.confirmStopService(displayName(item)),
        confirmLabel: L.manageStop,
        danger: true,
      });
      if (!ok) return;
    }
    try {
      await api.setServiceRunning(item.name, run);
      toast.success(run ? L.manageStartDone(displayName(item)) : L.manageStopDone(displayName(item)));
      await reload();
    } catch (e) {
      const msg = formatError(e);
      onError?.(msg);
      toast.error(msg);
      if (isAccessDeniedError(e)) {
        await ensureElevatedForManage();
      }
    }
  };

  const rows = useMemo(() => {
    const needle = q.trim().toLowerCase();
    return items.filter((it) => {
      if (onlyOn && !it.enabled && !(tab === "services" && it.running)) return false;
      if (hideMicrosoft && looksMicrosoft(it)) return false;
      if (!needle) return true;
      const program = it.task_action?.kind === "program" ? it.task_action.program : "";
      return [it.display_name, it.name, it.detail, it.path, it.location, program]
        .some((value) => value?.toLowerCase().includes(needle));
    });
  }, [items, q, onlyOn, hideMicrosoft, tab]);

  const virtualizer = useVirtualizer({
    count: tab === "tasks" ? 0 : rows.length,
    getScrollElement: () => tab === "tasks" ? null : scrollRef.current,
    estimateSize: () => ROW_H,
    overscan: 10,
  });
  const emptyLabel = !busy && (!progress || progress.status === "complete")
    ? (q.trim() || onlyOn || hideMicrosoft ? L.emptySearch : L.emptyList)
    : undefined;
  const searchPlaceholder = tab === "startup" ? L.searchStartup : tab === "services" ? L.searchServices : L.searchTasks;

  return (
    <div className={tab === "tasks" ? "remova-tasks" : undefined}
      style={{ ...css.card, flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}>
      <div className="remova-manage-toolbar">
        <input
          ref={searchRef}
          style={{ ...css.input, maxWidth: 380, height: 34, flex: "1 1 220px", minWidth: 0 }}
          aria-label={tab === "tasks" ? L.manageTaskSearch : searchPlaceholder}
          placeholder={searchPlaceholder}
          value={q}
          onChange={(e) => setQ(e.target.value)}
        />
        <label
          style={{ fontSize: 12, color: "var(--muted)", display: "flex", gap: 6, alignItems: "center" }}
        >
          <input type="checkbox" checked={onlyOn} onChange={(e) => setOnlyOn(e.target.checked)} />
          {L.filterOnlyOn}
        </label>
        {tab === "services" && (
          <label
            style={{ fontSize: 12, color: "var(--muted)", display: "flex", gap: 6, alignItems: "center" }}
          >
            <input
              type="checkbox"
              checked={hideMicrosoft}
              onChange={(e) => setHideMicrosoft(e.target.checked)}
            />
            {L.filterHideMs}
          </label>
        )}
        <div className="remova-manage-tools">
          {(!busy || items.length > 0) && <span className="remova-manage-count">
            {L.manageResultCount(rows.length, items.length)}
          </span>}
          {tab !== "tasks" && <button
            type="button"
            style={css.btnGhost}
            aria-expanded={helpOpen}
            aria-controls={helpId}
            onClick={() => setHelpOpen((open) => !open)}
          >{L.manageHelp}</button>}
          <button type="button" style={css.btnGhost} disabled={busy} onClick={() => void reload()}>
            {L.manageReload}
          </button>
        </div>
      </div>
      {tab !== "tasks" && <div id={helpId} className="remova-manage-help" hidden={!helpOpen}>
        {tab === "startup" ? L.startupHint : L.servicesHint}
      </div>}
      <div title={busy ? L.loadingManage : undefined}>
        <ScanProgressBar progress={progress} onCancel={cancel}
          stages={tab === "startup" ? ["preparing", "registry", "files", "tasks", "services"] : undefined} />
      </div>
      {tab === "tasks" ? (
        <TaskWorkspace rows={rows} scrollRef={scrollRef} busy={busy} emptyLabel={emptyLabel} onSetEnabled={setEnabled} />
      ) : <div
        ref={scrollRef}
        style={{ ...css.scroll, padding: "0 8px", position: "relative" }}
      >
        {emptyLabel && rows.length === 0 && (
          <div style={{ ...css.muted, padding: 28, textAlign: "center" as const }}>
            {emptyLabel}
          </div>
        )}
        {rows.length > 0 && (
          <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
            {virtualizer.getVirtualItems().map((vr) => {
              const it = rows[vr.index];
              if (!it) return null;
              const name = displayName(it);
              const source = tab === "startup" || it.system_service ? entrySource(it, tab, L) : "";
              const summary = entryDescription(it) || source;
              const statusTone = it.enabled ? "var(--ok-ink)" : "var(--muted)";
              return (
                <div
                  key={it.location + it.name}
                  className="remova-manage-row remova-manage-entry"
                  style={{
                    height: vr.size,
                    transform: `translateY(${vr.start}px)`,
                  }}
                >
                  <ManageItemIcon item={it} glyph={entryGlyph(it, tab)} />
                  <div style={{ flex: 1, minWidth: 0, overflow: "hidden" }}>
                    <div
                      style={{ fontWeight: 600, fontSize: 13 }}
                      className="ell"
                      title={name}
                    >
                      {name}
                    </div>
                    {summary && <div className="ell remova-manage-summary" title={summary}>
                      {summary}
                    </div>}
                  </div>
                  <div
                    style={{
                      display: "flex",
                      flexDirection: "column",
                      alignItems: "flex-end",
                      gap: 2,
                      flexShrink: 0,
                      overflow: "hidden",
                    }}
                  >
                    {tab === "startup" && (
                      <span
                        style={{
                          fontSize: 12,
                          fontWeight: 600,
                          color: statusTone,
                          whiteSpace: "nowrap",
                        }}
                      >
                        {enabledLabel(it, L)}
                      </span>
                    )}
                    {tab === "services" && (
                      <>
                        <span
                          style={{
                            fontSize: 12,
                            fontWeight: 600,
                            color: it.running ? "var(--ok-ink)" : "var(--muted)",
                            whiteSpace: "nowrap",
                          }}
                        >
                          {runLabel(it, L)}
                        </span>
                        <span style={{ ...css.muted, fontSize: 11, whiteSpace: "nowrap" }}>
                          {L.manageStartType}: {startTypeLabel(it, L)}
                        </span>
                      </>
                    )}
                  </div>
                  <div
                    style={{
                      display: "flex",
                      gap: 6,
                      flexShrink: 0,
                      alignItems: "center",
                    }}
                  >
                    {tab === "services" && (
                      <button
                        style={{
                          ...css.btnSm,
                          height: 32,
                          borderColor: it.running ? "var(--danger)" : "var(--border)",
                          color: it.running ? "var(--danger-text)" : "var(--accent)",
                        }}
                        disabled={busy}
                        title={it.running ? L.confirmStopService(name) : L.confirmStartService(name)}
                        aria-label={`${it.running ? L.manageStop : L.manageStartBtn}: ${name}`}
                        onClick={() => void setRunning(it, !it.running)}
                      >
                        {it.running ? L.manageStop : L.manageStartBtn}
                      </button>
                    )}
                    <button
                      style={{
                        ...css.btnSm,
                        height: 32,
                        borderColor: it.enabled ? "var(--border)" : "var(--accent)",
                        color: it.enabled ? "var(--muted)" : "var(--accent)",
                      }}
                      disabled={busy}
                      aria-label={`${it.enabled ? L.manageDisable : L.manageEnable}: ${name}`}
                      title={tab === "services" && it.enabled ? L.confirmDisableServiceVsStop(name) : undefined}
                      onClick={() => void setEnabled(it, !it.enabled)}
                    >
                      {it.enabled ? L.manageDisable : L.manageEnable}
                    </button>
                    <button
                      type="button"
                      className="remova-manage-more"
                      aria-label={`${L.manageItemDetails} · ${name}`}
                      title={L.manageItemDetails}
                      aria-haspopup="dialog"
                      onClick={() => {
                        setDetailsItem(it);
                        detailsRef.current?.showModal();
                      }}
                    >…</button>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>}
      {tab !== "tasks" && <dialog ref={detailsRef} className="remova-manage-dialog" aria-labelledby={detailsTitleId}>
        <header>
          {detailsItem && <ManageItemIcon item={detailsItem} glyph={entryGlyph(detailsItem, tab)} />}
          <h2 id={detailsTitleId}>{detailsItem && displayName(detailsItem)}</h2>
          <form method="dialog"><button type="submit" style={css.btnGhost}>{L.panelClose}</button></form>
        </header>
        {detailsItem && <>
          {entryDescription(detailsItem) && <p>{entryDescription(detailsItem)}</p>}
          <dl>
            <dt>{L.manageDetailSource}</dt><dd>{entrySource(detailsItem, tab, L)}</dd>
            <dt>{L.manageDetailId}</dt><dd>{detailsItem.name}</dd>
            {detailsItem.path && <><dt>{L.manageDetailProgram}</dt><dd>{detailsItem.path}</dd></>}
            <dt>{L.manageDetailLocation}</dt><dd>{detailsItem.location}</dd>
          </dl>
        </>}
      </dialog>}
    </div>
  );
}
