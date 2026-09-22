import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../lib/api";
import { useVirtualizer } from "@tanstack/react-virtual";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError, isAccessDeniedError } from "../lib/format";
import { requestConfirm } from "../lib/confirm";
import { toast } from "../lib/toast";
import { useManageList, type ManageTabId } from "../lib/useManageList";
import { looksMicrosoft } from "../lib/manageFilter";
import type { ManageItem } from "../types";

export type ManagePageTab = ManageTabId;

const ROW_H = 72;

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
  if (it.source_label) return it.source_label;
  const loc = it.location || "";
  if (loc.startsWith("PACKAGED::")) return L.manageSourceStore;
  if (loc.startsWith("SVC::")) return L.manageSourceService;
  if (loc.startsWith("FOLDER::")) return L.manageSourceFolder;
  return L.manageSourceRegistry;
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
  title,
  onError,
}: {
  tab: ManagePageTab;
  title: string;
  onError?: (msg: string) => void;
}) {
  const L = t();
  const [q, setQ] = useState("");
  const [onlyOn, setOnlyOn] = useState(true);
  const [hideMicrosoft, setHideMicrosoft] = useState(tab === "services");
  const scrollRef = useRef<HTMLDivElement | null>(null);

  const { items, busy, reload } = useManageList(tab, onError);

  useEffect(() => {
    setHideMicrosoft(tab === "services");
  }, [tab]);

  const setEnabled = async (item: ManageItem, nextEnabled: boolean) => {
    if (!(await ensureElevatedForManage())) {
      return;
    }
    if (!nextEnabled) {
      const message =
        tab === "services"
          ? L.confirmDisableServiceVsStop(item.name)
          : tab === "tasks"
            ? L.confirmDisableTask(item.name)
            : L.confirmDisableStartup(item.name);
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
      toast.success(nextEnabled ? L.manageEnabledDone(item.name) : L.manageDisabledDone(item.name));
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
        message: L.confirmStartService(item.name),
        confirmLabel: L.manageStartBtn,
      });
      if (!ok) return;
    } else {
      const ok = await requestConfirm({
        title: L.manageStop,
        message: L.confirmStopService(item.name),
        confirmLabel: L.manageStop,
        danger: true,
      });
      if (!ok) return;
    }
    try {
      await api.setServiceRunning(item.name, run);
      toast.success(run ? L.manageStartDone(item.name) : L.manageStopDone(item.name));
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
      return (
        it.name.toLowerCase().includes(needle) ||
        (it.detail || "").toLowerCase().includes(needle) ||
        (it.location || "").toLowerCase().includes(needle)
      );
    });
  }, [items, q, onlyOn, hideMicrosoft, tab]);

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_H,
    overscan: 10,
  });

  return (
    <div style={{ ...css.card, flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}>
      <div
        style={{
          display: "flex",
          gap: 10,
          alignItems: "center",
          padding: "12px 14px",
          borderBottom: "1px solid var(--border)",
          flexShrink: 0,
          flexWrap: "wrap",
        }}
      >
        <strong style={{ fontSize: 14 }}>{title}</strong>
        <span style={css.chip}>
          {busy && items.length === 0
            ? "…"
            : `${rows.length}${rows.length !== items.length ? ` / ${items.length}` : ""}`}
        </span>
        <input
          style={{ ...css.input, maxWidth: 240, height: 34, flex: "0 1 240px" }}
          placeholder={
            tab === "startup"
              ? L.searchStartup
              : tab === "services"
                ? L.searchServices
                : L.searchTasks
          }
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
        <button
          style={{ ...css.btnGhost, marginLeft: "auto" }}
          disabled={busy}
          onClick={() => void reload()}
        >
          {L.manageReload}
        </button>
      </div>
      <div
        style={{
          padding: "8px 14px",
          fontSize: 12,
          color: "var(--muted)",
          background: "var(--surface-2)",
          borderBottom: "1px solid var(--border)",
          lineHeight: 1.5,
          flexShrink: 0,
        }}
      >
        {tab === "startup" ? L.startupHint : tab === "services" ? L.servicesHint : L.tasksHint}
      </div>
      <div
        ref={scrollRef}
        style={{ ...css.scroll, padding: "4px 8px", position: "relative" }}
      >
        {busy && items.length === 0 && (
          <div style={{ ...css.muted, padding: 16 }}>{L.loadingManage}</div>
        )}
        {!busy && rows.length === 0 && (
          <div style={{ ...css.muted, padding: 28, textAlign: "center" as const }}>
            {L.emptyList}
          </div>
        )}
        {rows.length > 0 && (
          <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
            {virtualizer.getVirtualItems().map((vr) => {
              const it = rows[vr.index];
              if (!it) return null;
              const statusTone = it.enabled ? "var(--ok)" : "var(--muted)";
              return (
                <div
                  key={it.location + it.name}
                  style={{
                    position: "absolute",
                    top: 0,
                    left: 0,
                    width: "100%",
                    height: vr.size,
                    transform: `translateY(${vr.start}px)`,
                    display: "flex",
                    alignItems: "center",
                    gap: 12,
                    padding: "0 10px",
                    borderBottom: "1px solid var(--border)",
                    boxSizing: "border-box",
                  }}
                >
                  <div
                    style={{
                      width: 36,
                      height: 36,
                      borderRadius: 10,
                      background: "var(--surface-2)",
                      color: "var(--muted)",
                      display: "grid",
                      placeItems: "center",
                      fontSize: 14,
                      flexShrink: 0,
                    }}
                  >
                    {tab === "startup" ? "⚡" : tab === "services" ? "⚙" : "⏱"}
                  </div>
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div
                      style={{ fontWeight: 600, fontSize: 13.5 }}
                      className="ell"
                      title={it.name}
                    >
                      {it.name}
                    </div>
                    <div
                      style={{ ...css.muted, fontSize: 11.5, wordBreak: "break-all" }}
                      className="ell"
                      title={`${it.detail || ""}\n${it.location || ""}`}
                    >
                      {tab === "startup"
                        ? startupSourceLabel(it, L)
                        : tab === "services"
                          ? [
                              it.source_label || it.name,
                              `${L.manageStartType}: ${startTypeLabel(it, L)}`,
                              it.path || it.detail || it.location,
                            ]
                              .filter(Boolean)
                              .join(" · ")
                          : it.detail || it.location}
                    </div>
                    {tab === "tasks" && (
                      <div style={{ ...css.muted, fontSize: 11 }} className="ell">
                        {L.manageLastRun}: {it.last_run || L.manageNoLastRun}
                        {" · "}
                        {L.manageNextRun}: {it.next_run || L.manageNoLastRun}
                      </div>
                    )}
                  </div>
                  <div
                    style={{
                      display: "flex",
                      flexDirection: "column",
                      alignItems: "flex-end",
                      gap: 2,
                      minWidth: tab === "services" ? 110 : 72,
                      flexShrink: 0,
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
                            color: it.running ? "var(--ok)" : "var(--muted)",
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
                    {tab === "tasks" && (
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
                          color: it.running ? "var(--danger)" : "var(--accent)",
                        }}
                        disabled={busy}
                        title={it.running ? L.confirmStopService(it.name) : L.confirmStartService(it.name)}
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
                      title={
                        tab === "services" && it.enabled
                          ? L.confirmDisableServiceVsStop(it.name)
                          : undefined
                      }
                      onClick={() => void setEnabled(it, !it.enabled)}
                    >
                      {it.enabled ? L.manageDisable : L.manageEnable}
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
