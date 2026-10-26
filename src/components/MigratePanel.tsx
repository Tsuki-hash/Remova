import { useCallback, useEffect, useRef, useState } from "react";
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
  source_unreadable: "migrateRejSourceUnreadable",
  source_hardlinks: "migrateRejHardlinks",
  target_not_enough_space: "migrateRejNoSpace",
};

function rejectionText(L: ReturnType<typeof t>, code: string): string {
  const key = REJECTION_TEXT[code];
  return key ? (L[key as keyof typeof L] as string) : code;
}

/**
 * Junction directory migration (M6.3): Plan → Copy → Cutover → Finish, each
 * phase a separate button gated by the server-derived state; Rollback is
 * offered whenever the cutover is in place. Only-move: the source tree
 * survives as `<source>.bak` until Finish verifies it away.
 */
export function MigratePanel({ onClose, initialSource = "" }: { onClose: () => void; initialSource?: string }) {
  const L = t();
  const [source, setSource] = useState(initialSource);
  const [target, setTarget] = useState("");
  const [plan, setPlan] = useState<MigrationPlan | null>(null);
  const [state, setState] = useState<MigrationState | null>(null);
  const [busy, setBusy] = useState(false);
  // True only while a migrateCopy IPC is actually in flight. The copy is the
  // one phase the backend can abort (COPY_CANCEL), so the cancel button keys
  // off it instead of the derived wizard state, which does not change until
  // the copy settles.
  const [copying, setCopying] = useState(false);
  // Monotonic id for status probes: the probe runs on the blocking pool shared
  // with every scan, so a slow answer for an older path pair can arrive after
  // a newer one and must never overwrite its state.
  const statusGen = useRef(0);

  const refreshState = useCallback(async (src: string, dst: string) => {
    if (!src.trim() || !dst.trim()) return;
    const gen = ++statusGen.current;
    try {
      const next = await api.migrateStatus(src, dst);
      if (gen === statusGen.current) setState(next);
    } catch {
      if (gen === statusGen.current) setState(null);
    }
  }, []);

  useEffect(() => {
    // Debounce: every keystroke used to fire a status IPC (each one a
    // spawn_blocking + filesystem probe).
    const timer = setTimeout(() => void refreshState(source, target), 300);
    return () => clearTimeout(timer);
  }, [source, target, refreshState]);

  const withBusy = async (work: () => Promise<void>) => {
    if (busy) return;
    setBusy(true);
    try {
      await work();
    } finally {
      setBusy(false);
    }
  };

  // The four phase actions are plain async functions; the call sites below are
  // their single withBusy wrapper.
  const startMigration = async () => {
    const plan = await api.migratePlan(source, target);
    setPlan(plan);
    if (!plan.ok) return;
    setCopying(true);
    try {
      const r = await api.migrateCopy(source, target);
      toast.success(
        L.migrateCopyDone(r.files, formatSize(r.source_kb)),
        r.reparse_skipped.length ? { detail: L.migrateCopySkipped(r.reparse_skipped.length) } : undefined,
      );
    } catch (e) {
      // A cancel is the user's own choice, not a failure. Check the raw error:
      // formatError would already localize it, hiding the token.
      const raw = typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
      if (raw === "migrate:cancelled") toast.info(L.migrateCancelled);
      else toast.error(backendErrorText(formatError(e)));
    } finally {
      setCopying(false);
    }
    await refreshState(source, target);
  };

  const cancelCopy = () => {
    // Not gated by busy: the button exists precisely to interrupt the busy
    // copy, so the click must reach the backend even while busy is true.
    void api.cancelMigrateCopy().catch(() => {});
  };

  const runCutover = async () => {
    const { ok } = await requestConfirmEx({
      title: L.migrateCutoverTitle,
      message: L.migrateCutoverConfirm,
      confirmLabel: L.migrateCutover,
      danger: true,
    });
    if (!ok) return;
    const r = await api.migrateCutover(source, target);
    if (r.ok) {
      toast.success(L.migrateCutoverDone);
    } else if (r.rolled_back) {
      toast.error(L.migrateRolledBack, { detail: r.error ?? undefined });
    } else {
      toast.error(L.migrateCutoverFailed, { detail: r.error ?? undefined });
    }
    await refreshState(source, target);
  };

  const runFinish = async () => {
    const { ok } = await requestConfirmEx({
      title: L.migrateFinishTitle,
      message: L.migrateFinishConfirm,
      confirmLabel: L.migrateFinish,
      danger: true,
    });
    if (!ok) return;
    try {
      const r = await api.migrateFinish(source, target, true);
      if (r.removed_bak) {
        toast.success(L.migrateFinishDone);
      } else {
        toast.error(L.migrateFinishRefused, {
          detail: r.missing_in_target.slice(0, 10).join("\n"),
        });
      }
    } catch (e) {
      toast.error(backendErrorText(formatError(e)));
    }
    await refreshState(source, target);
  };

  const runRollback = async () => {
    try {
      const r = await api.migrateRollback(source, target);
      if (r.ok) {
        toast.success(L.migrateRollbackDone);
      } else {
        // The same string is localized for the finish path; the rollback
        // report deserves the same treatment.
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
        <strong>{L.migrateTitle}</strong>
        <span style={css.muted}>{L.migrateHint}</span>
        <button style={{ ...css.btnGhost, height: 28, marginLeft: "auto" }} onClick={onClose} aria-label={L.panelClose}>
          <CloseGlyph />
        </button>
      </div>

      <label style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 6 }}>
        <span style={{ ...css.muted, fontSize: 12, minWidth: 96 }}>{L.migrateSource}</span>
        <input style={{ ...css.input, height: 30 }} value={source} onChange={(e) => setSource(e.target.value)}
          placeholder="C:\\Users\\me\\AppData\\Local\\SomeApp" />
      </label>
      <label style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 6 }}>
        <span style={{ ...css.muted, fontSize: 12, minWidth: 96 }}>{L.migrateTarget}</span>
        <input style={{ ...css.input, height: 30 }} value={target} onChange={(e) => setTarget(e.target.value)}
          placeholder="D:\\Moved\\SomeApp" />
      </label>

      {(() => {
        const steps = [L.stepPlan, L.stepCopy, L.stepCutover, L.stepFinish];
        // The current step is the action the user takes NEXT: after a copy
        // the cutover is next; after a cutover the finish is next.
        const idx = state === "cutover" ? 3 : state === "copied" ? 2 : state === "finished" ? 4 : 0;
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
          type="button"
          style={{ ...css.btn, height: 34, opacity: busy ? 0.5 : 1 }}
          disabled={busy || !source.trim() || !target.trim() || state === "finished"}
          onClick={() => {
            if (state === "copied") void withBusy(runCutover);
            else if (state === "cutover") void withBusy(runFinish);
            else void withBusy(startMigration);
          }}
        >
          {busy
            ? L.migrateWorking
            : state === "copied"
              ? L.migrateCutover
              : state === "cutover"
                ? L.migrateFinish
                : L.migrateStart}
        </button>
        {state === "cutover" && (
          <button type="button" style={{ ...css.btnGhost, height: 34 }} disabled={busy} onClick={() => void withBusy(runRollback)}>
            {L.migrateRollback}
          </button>
        )}
        {state === "cutover" && (
          <span style={{ ...css.muted, fontSize: 12 }}>{L.migrateFinishLater}</span>
        )}
        {copying && (
          <button type="button" style={{ ...css.btnGhost, height: 30 }} onClick={cancelCopy}>
            {L.migrateCancelCopy}
          </button>
        )}
      </div>

      {state === "inconsistent" && (
        <div role="alert" style={{ color: "var(--danger-text)", fontSize: 12, marginTop: 6 }}>
          {L.migrateStateInconsistent}
        </div>
      )}

      {plan && (
        <div style={{ marginTop: 10, borderTop: "1px solid var(--border)", paddingTop: 8 }}>
          {plan.ok ? (
            <div role="status">
              {L.migratePlanOk(formatSize(plan.source_kb), String(plan.file_count), formatSize(plan.target_free_kb))}
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
