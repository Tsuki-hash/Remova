import { useCallback, useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from "react";
import { api } from "../lib/api";
import { t, formatSize } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
import { backendErrorText } from "../lib/backendText";
import { requestConfirmEx } from "../lib/confirm";
import { toast } from "../lib/toast";
import { CloseGlyph } from "./ui/Glyph";
import type { MigrationPlan, MigrationState } from "../types";

const REJECTION_TEXT: Record<string, string> = {
  source_missing: "migrateRejSourceMissing",
  source_is_reparse: "migrateRejSourceReparse",
  same_path: "migrateRejSamePath",
  nested_paths: "migrateRejNested",
  source_protected_tree: "migrateRejProtected",
  source_msix: "migrateRejMsix",
  source_squirrel_updater: "migrateRejSquirrel",
  backup_path_taken: "migrateRejBackupTaken",
  target_not_local_fixed: "migrateRejTargetDrive",
  target_parent_missing: "migrateRejTargetParent",
  target_is_reparse: "migrateRejTargetReparse",
  target_not_empty: "migrateRejTargetNotEmpty",
  source_too_many_files: "migrateRejTooMany",
  source_tree_too_deep: "migrateRejTreeTooDeep",
  target_directory_permissions_differ: "migrateRejDirectoryPermissionsDiffer",
  source_unreadable: "migrateRejSourceUnreadable",
  source_hardlinks: "migrateRejHardlinks",
  source_not_local_fixed: "migrateRejSourceDrive",
  source_ancestor_reparse: "migrateRejAncestorReparse",
  target_ancestor_reparse: "migrateRejAncestorReparse",
  target_protected_tree: "migrateRejTargetProtected",
  target_not_enough_space: "migrateRejNoSpace",
  target_space_unknown: "migrateRejSpaceUnknown",
};

const CLEANUP_PENDING_PREFIX = "migrate:cleanup_pending:";

type MigrationOperation = {
  source: string;
  target: string;
  copying: boolean;
  copyId: number | null;
  cancelRequested: boolean;
};

// An operation can outlive the panel or the entire More page.
let activeMigration: MigrationOperation | null = null;
const migrationListeners = new Set<() => void>();
const getActiveMigration = () => activeMigration;
function subscribeMigration(listener: () => void) {
  migrationListeners.add(listener);
  return () => { migrationListeners.delete(listener); };
}
function setActiveMigration(operation: MigrationOperation | null) {
  activeMigration = operation;
  for (const listener of migrationListeners) listener();
}

function rejectionText(L: ReturnType<typeof t>, code: string): string {
  const key = REJECTION_TEXT[code];
  return key ? (L[key as keyof typeof L] as string) : code;
}

/**
 * Junction directory migration: Plan → Copy → Cutover → Finish, each
 * phase a separate button gated by the server-derived state; Rollback is
 * offered whenever the cutover is in place. Only-move: the source tree
 * survives as `<source>.bak` until Finish verifies it away.
 */
export function MigratePanel({ onClose, initialSource = "" }: { onClose: () => void; initialSource?: string }) {
  const L = t();
  const operation = useSyncExternalStore(subscribeMigration, getActiveMigration);
  const [sourceInput, setSource] = useState(() => operation?.source ?? initialSource);
  const [targetInput, setTarget] = useState(() => operation?.target ?? "");
  const source = operation?.source ?? sourceInput;
  const target = operation?.target ?? targetInput;
  const [plan, setPlan] = useState<MigrationPlan | null>(null);
  const [state, setState] = useState<MigrationState | null>(null);
  const [actionFocus, setActionFocus] = useState<HTMLButtonElement | null>(null);
  const primaryAction = useRef<HTMLButtonElement>(null);
  const title = useRef<HTMLElement>(null);
  const busy = operation !== null;
  const reopenedDuringOperation = useRef(busy);
  // Monotonic id for status probes: the probe runs on the blocking pool shared
  // with every scan, so a slow answer for an older path pair can arrive after
  // a newer one and must never overwrite its state.
  const statusGen = useRef(0);

  const editPath = (setPath: (value: string) => void, value: string) => {
    if (activeMigration) return;
    ++statusGen.current;
    setState(null);
    setPlan(null);
    setPath(value);
  };

  const refreshState = useCallback(async (src: string, dst: string, fallback: MigrationState | null = null) => {
    if (!src.trim() || !dst.trim()) return;
    const gen = ++statusGen.current;
    try {
      const next = await api.migrateStatus(src, dst);
      if (gen === statusGen.current) setState(next);
    } catch {
      if (gen === statusGen.current) setState(fallback);
    }
  }, []);

  useEffect(() => {
    // Debounce: every keystroke used to fire a status IPC (each one a
    // spawn_blocking + filesystem probe).
    const timer = setTimeout(() => void refreshState(source, target), 300);
    return () => clearTimeout(timer);
  }, [source, target, refreshState]);

  useEffect(() => {
    if (!busy && reopenedDuringOperation.current) {
      reopenedDuringOperation.current = false;
      void refreshState(source, target);
    }
  }, [busy, source, target, refreshState]);

  useLayoutEffect(() => {
    if (busy || !actionFocus) return;
    setActionFocus(null);
    const doc = actionFocus.ownerDocument;
    if (doc.activeElement !== doc.body) return;
    if (actionFocus.isConnected && !actionFocus.disabled) {
      actionFocus.focus();
    } else if (primaryAction.current && !primaryAction.current.disabled) {
      primaryAction.current.focus();
    } else {
      title.current?.focus();
    }
  }, [actionFocus, busy]);

  const withBusy = async (opener: HTMLButtonElement, work: () => Promise<void>) => {
    if (activeMigration) return;
    setActiveMigration({ source, target, copying: false, copyId: null, cancelRequested: false });
    try {
      await work();
    } finally {
      setActiveMigration(null);
      setActionFocus(opener);
    }
  };

  // The four phase actions are plain async functions; the call sites below are
  // their single withBusy wrapper.
  const startMigration = async () => {
    let plan: import("../types").MigrationPlan;
    try {
      plan = await api.migratePlan(source, target);
    } catch (e) {
      toast.error(backendErrorText(formatError(e)));
      return;
    }
    setPlan(plan);
    if (!plan.ok) return;
    if (activeMigration) setActiveMigration({ ...activeMigration, copying: true });
    let copyId: number | null = null;
    try {
      copyId = await api.beginMigrateCopy(source, target);
      if (activeMigration) setActiveMigration({ ...activeMigration, copyId });
      const r = await api.migrateCopy(copyId);
      toast.success(
        L.migrateCopyDone(r.files, formatSize(r.source_kb)),
        r.reparse_skipped.length ? { detail: L.migrateCopySkipped(r.reparse_skipped.length) } : undefined,
      );
    } catch (e) {
      // A cancel is the user's own choice, not a failure. Check the raw error:
      // formatError would already localize it, hiding the token.
      const raw = typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
      if (raw === "migrate:cancelled") toast.info(L.migrateCancelled);
      else {
        if (copyId !== null) void api.cancelMigrateCopy(copyId).catch(() => {});
        toast.error(backendErrorText(formatError(e)));
      }
    } finally {
      if (activeMigration) setActiveMigration({ ...activeMigration, copyId: null });
    }
    await refreshState(source, target);
  };

  const cancelCopy = () => {
    const operation = activeMigration;
    if (!operation || operation.copyId === null || operation.cancelRequested) return;
    const { copyId } = operation;
    setActiveMigration({ ...operation, cancelRequested: true });
    void api.cancelMigrateCopy(copyId).catch((e) => {
      if (activeMigration?.copyId !== copyId) return;
      setActiveMigration({ ...activeMigration, cancelRequested: false });
      toast.error(backendErrorText(formatError(e)));
    });
  };

  const confirmPaths = (message: string) => `${message}\n${L.migrateSource}: ${source}\n${L.migrateTarget}: ${target}`;
  const cleanupPending = state === "cleanup_pending";

  const runCutover = async () => {
    const { ok } = await requestConfirmEx({
      title: L.migrateCutoverTitle,
      message: confirmPaths(L.migrateCutoverConfirm),
      confirmLabel: L.migrateCutover,
      danger: true,
    });
    if (!ok) return;
    try {
      const r = await api.migrateCutover(source, target);
      if (r.ok) {
        toast.success(L.migrateCutoverDone);
      } else if (r.rolled_back) {
        toast.error(L.migrateRolledBack, {
          detail: r.error ? backendErrorText(r.error) : undefined,
        });
      } else {
        toast.error(L.migrateCutoverFailed, {
          detail: r.error ? backendErrorText(r.error) : undefined,
        });
      }
    } catch (e) {
      // An IPC-level rejection (worker died, command unavailable) must say so
      // and still leave the panel on a fresh state, or the user is stuck on a
      // stale step with no way forward.
      toast.error(backendErrorText(formatError(e)));
    }
    await refreshState(source, target);
  };

  const runFinish = async () => {
    const { ok } = await requestConfirmEx({
      title: cleanupPending ? L.migrateRetryCleanup : L.migrateFinishTitle,
      message: confirmPaths(cleanupPending ? L.migrateRetryCleanupConfirm : L.migrateFinishConfirm),
      confirmLabel: cleanupPending ? L.migrateRetryCleanup : L.migrateFinish,
      danger: true,
    });
    if (!ok) return;
    let fallback: MigrationState | null = null;
    try {
      const r = await api.migrateFinish(source, target, true);
      if (r.removed_bak) {
        fallback = "finished";
        toast.success(L.migrateFinishDone);
      } else {
        toast.error(L.migrateFinishRefused, {
          detail: r.missing_in_target.slice(0, 10).join("\n"),
        });
      }
    } catch (e) {
      const raw = typeof e === "string" ? e : e instanceof Error ? e.message : "";
      if (raw.startsWith(CLEANUP_PENDING_PREFIX)) {
        fallback = "cleanup_pending";
        toast.error(L.migrateCleanupPending, { detail: raw.slice(CLEANUP_PENDING_PREFIX.length).trim() });
      } else {
        toast.error(backendErrorText(formatError(e)));
      }
    }
    if (fallback) setState(fallback);
    await refreshState(source, target, fallback);
  };

  const runRollback = async () => {
    const { ok } = await requestConfirmEx({
      title: L.migrateRollback,
      message: confirmPaths(L.migrateRollbackConfirm),
      confirmLabel: L.migrateRollback,
      danger: true,
    });
    if (!ok) return;
    try {
      const r = await api.migrateRollback(source, target);
      if (r.ok) {
        toast.success(L.migrateRollbackDone);
      } else {
        toast.error(L.migrateRollbackFailed, {
          detail: r.message ? backendErrorText(r.message) : undefined,
        });
      }
    } catch (e) {
      toast.error(backendErrorText(formatError(e)));
    }
    await refreshState(source, target);
  };


  return (
    <section style={{ ...css.card, marginBottom: 16, padding: 16, fontSize: 13 }} aria-label={L.migrateTitle}>
      <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap", marginBottom: 8 }}>
        <strong ref={title} tabIndex={-1}>{L.migrateTitle}</strong>
        <span style={css.muted}>{L.migrateHint}</span>
        <button style={{ ...css.btnGhost, height: 28, marginLeft: "auto" }} onClick={onClose} aria-label={L.panelClose}>
          <CloseGlyph />
        </button>
      </div>

      <label style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 6 }}>
        <span style={{ ...css.muted, fontSize: 12, minWidth: 96 }}>{L.migrateSource}</span>
        <input style={{ ...css.input, height: 30 }} value={source} disabled={busy}
          onChange={(e) => editPath(setSource, e.target.value)}
          placeholder="C:\\Users\\me\\AppData\\Local\\SomeApp" />
      </label>
      <label style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 6 }}>
        <span style={{ ...css.muted, fontSize: 12, minWidth: 96 }}>{L.migrateTarget}</span>
        <input style={{ ...css.input, height: 30 }} value={target} disabled={busy}
          onChange={(e) => editPath(setTarget, e.target.value)}
          placeholder="D:\\Moved\\SomeApp" />
      </label>

      {(() => {
        const steps = [L.stepPlan, L.stepCopy, L.stepCutover, L.stepFinish];
        // The current step is the action the user takes NEXT: after a copy
        // the cutover is next; after a cutover the finish is next.
        const idx = operation?.copying ? 1 : state === "cutover" || cleanupPending ? 3 : state === "copied" ? 2 : state === "finished" ? 4 : 0;
        return (
          <ol aria-label={L.migrateTitle} style={{ display: "flex", gap: 6, flexWrap: "wrap", listStyle: "none", margin: "8px 0", padding: 0 }}>
            {steps.map((label, i) => {
              const done = i < idx;
              const current = i === idx;
              return (
                <li
                  key={label}
                  aria-current={current ? "step" : undefined}
                  style={{
                    display: "inline-flex",
                    alignItems: "center",
                    gap: 6,
                    fontSize: 12,
                    padding: "3px 10px",
                    borderRadius: 999,
                    border: current ? "1px solid var(--accent)" : "1px solid var(--border)",
                    color: done ? "var(--ok-ink)" : current ? "var(--accent-text)" : "var(--muted)",
                    fontWeight: current ? 650 : 500,
                    background: current ? "var(--accent-soft)" : "transparent",
                  }}
                >
                  {done ? "\u2713 " : ""}{i + 1}. {label}
                </li>
              );
            })}
          </ol>
        );
      })()}

      <div style={{ display: "flex", gap: 8, flexWrap: "wrap", alignItems: "center", marginTop: 6, justifyContent: "flex-end" }}>
        <button
          ref={primaryAction}
          type="button"
          style={{ ...css.btn, height: 34, opacity: busy ? 0.5 : 1 }}
          disabled={busy || !source.trim() || !target.trim() || state === "finished"}
          onClick={({ currentTarget }) => {
            if (state === "copied") void withBusy(currentTarget, runCutover);
            else if (state === "cutover" || cleanupPending) void withBusy(currentTarget, runFinish);
            else void withBusy(currentTarget, startMigration);
          }}
        >
          {busy
            ? L.migrateWorking
            : state === "copied"
              ? L.migrateCutover
              : cleanupPending
                ? L.migrateRetryCleanup
                : state === "cutover" ? L.migrateFinish : L.migrateStart}
        </button>
        {state === "cutover" && (
          <button type="button" style={{ ...css.btnGhost, height: 34 }} disabled={busy} onClick={({ currentTarget }) => void withBusy(currentTarget, runRollback)}>
            {L.migrateRollback}
          </button>
        )}
        {state === "cutover" && (
          <span style={{ ...css.muted, fontSize: 12 }}>{L.migrateFinishLater}</span>
        )}
        {operation?.copyId != null && (
          <button type="button" style={{ ...css.btnGhost, height: 30 }} disabled={operation.cancelRequested} onClick={cancelCopy}>
            {L.migrateCancelCopy}
          </button>
        )}
      </div>

      {state === "inconsistent" && (
        <div role="alert" style={{ color: "var(--danger-text)", fontSize: 12, marginTop: 6 }}>
          {L.migrateStateInconsistent}
        </div>
      )}
      {cleanupPending && (
        <div role="alert" style={{ color: "var(--warn-ink)", fontSize: 12, marginTop: 6 }}>
          {L.migrateCleanupPending}
        </div>
      )}

      {plan && (
        <div style={{ marginTop: 10, borderTop: "1px solid var(--border)", paddingTop: 8 }}>
          {plan.ok ? (
            <div role="status">
              {L.migratePlanOk(formatSize(plan.source_kb), String(plan.file_count), formatSize(plan.target_free_kb))}
              {plan.root_permissions_pending && (
                <div style={{ ...css.muted, fontSize: 12, marginTop: 2 }}>{L.migrateRootPermissionsPending}</div>
              )}
              {plan.source_kb_capped && (
                <div style={{ ...css.muted, fontSize: 12, marginTop: 2 }}>{L.migratePlanEstimate}</div>
              )}
            </div>
          ) : (
            <div role="alert" style={{ color: "var(--danger-text)" }}>
              <div style={{ fontWeight: 600 }}>{L.migratePlanRefused}</div>
              {plan.rejections.map((r) => (
                <div key={r} style={{ ...css.muted, marginTop: 2 }}>{rejectionText(L, r)}</div>
              ))}
            </div>
          )}
        </div>
      )}

      <div style={{ ...css.muted, fontSize: 12, marginTop: 10 }}>{L.migrateNote}</div>
    </section>
  );
}
