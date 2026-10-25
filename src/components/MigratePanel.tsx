import { useCallback, useEffect, useState } from "react";
import { api } from "../lib/api";
import { t, formatSize } from "../i18n";
import { cssStyles as css } from "../styles";
import { formatError } from "../lib/format";
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
export function MigratePanel({ onClose }: { onClose: () => void }) {
  const L = t();
  const [source, setSource] = useState("");
  const [target, setTarget] = useState("");
  const [plan, setPlan] = useState<MigrationPlan | null>(null);
  const [state, setState] = useState<MigrationState | null>(null);
  const [busy, setBusy] = useState(false);

  const refreshState = useCallback(async (src: string, dst: string) => {
    if (!src.trim() || !dst.trim()) return;
    try {
      setState(await api.migrateStatus(src, dst));
    } catch {
      setState(null);
    }
  }, []);

  useEffect(() => {
    void refreshState(source, target);
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

  const runPlan = () =>
    withBusy(async () => {
      setPlan(await api.migratePlan(source, target));
    });

  const runCopy = () =>
    withBusy(async () => {
      try {
        const r = await api.migrateCopy(source, target);
        toast.success(
          L.migrateCopyDone(r.files, formatSize(r.source_kb)),
          r.reparse_skipped.length ? { detail: L.migrateCopySkipped(r.reparse_skipped.length) } : undefined,
        );
      } catch (e) {
        toast.error(formatError(e));
      }
      await refreshState(source, target);
    });

  const cancelCopy = () =>
    withBusy(async () => {
      await api.cancelMigrateCopy();
    });

  const runCutover = () =>
    withBusy(async () => {
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
    });

  const runFinish = () =>
    withBusy(async () => {
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
        toast.error(formatError(e));
      }
      await refreshState(source, target);
    });

  const runRollback = () =>
    withBusy(async () => {
      try {
        const r = await api.migrateRollback(source, target);
        if (r.ok) {
          toast.success(L.migrateRollbackDone);
        } else {
          toast.error(L.migrateRollbackFailed, { detail: r.message ?? undefined });
        }
      } catch (e) {
        toast.error(formatError(e));
      }
      await refreshState(source, target);
    });

  const stateLabel: Record<MigrationState, string> = {
    absent: L.migrateStateAbsent,
    idle: L.migrateStateIdle,
    copied: L.migrateStateCopied,
    cutover: L.migrateStateCutover,
    finished: L.migrateStateFinished,
    inconsistent: L.migrateStateInconsistent,
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

      {state && state !== "absent" && state !== "idle" && (
        <div role="status" style={{ margin: "6px 0" }}>
          {L.migrateCurrentState(stateLabel[state])}
        </div>
      )}

      <div style={{ display: "flex", gap: 8, flexWrap: "wrap", marginTop: 6 }}>
        <button type="button" style={{ ...css.btnGhost, height: 30 }} disabled={busy || !source.trim() || !target.trim()} onClick={() => void runPlan()}>
          {L.migratePlanBtn}
        </button>
        <button type="button" style={{ ...css.btnGhost, height: 30 }}
          disabled={busy || !plan?.ok || state !== "copied"} onClick={() => void runCopy()}>
          {L.migrateCopyBtn}
        </button>
        <button type="button" style={{ ...css.btnGhost, height: 30 }} disabled={busy || state !== "copied"} onClick={() => void runCutover()}>
          {L.migrateCutover}
        </button>
        <button type="button" style={{ ...css.btnGhost, height: 30 }} disabled={busy || state !== "cutover"} onClick={() => void runFinish()}>
          {L.migrateFinish}
        </button>
        <button type="button" style={{ ...css.btnGhost, height: 30 }} disabled={busy || state !== "cutover"} onClick={() => void runRollback()}>
          {L.migrateRollback}
        </button>
        {busy && state === "copied" && (
          <button type="button" style={{ ...css.btnGhost, height: 30 }} onClick={() => void cancelCopy()}>
            {L.cancelScan}
          </button>
        )}
      </div>

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
