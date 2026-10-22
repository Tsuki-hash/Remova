import { useState } from "react";
import { api } from "../lib/api";
import { formatError } from "../lib/format";
import { gateReasonText } from "../lib/decision";
import { backendErrorText } from "../lib/backendText";
import { requestConfirmEx } from "../lib/confirm";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import type { ItemDetail, LockingProcess } from "../types";

type LockState = { procs: LockingProcess[]; error?: string; busyPid?: number };

function imageUnderRoot(image: string | undefined, root: string | undefined): boolean {
  if (!image || !root) return false;
  const img = image.replace(/\//g, "\\").toLowerCase();
  const base = root.replace(/\//g, "\\").toLowerCase().replace(/\\+$/, "");
  const segments = img.split("\\");
  return img.endsWith(".exe")
    && !segments.includes("..")
    && !segments.includes(".")
    && img.startsWith(`${base}\\`);
}

function reason(message: string) {
  const L = t();
  const known = gateReasonText(message, L);
  if (known !== L.backendDetailUnavailable) return known;
  if (/os error (32|33)\b|sharing violation|being used by another process|另一个进程|另一进程/i.test(message)) return L.reportFileBusy;
  if (/os error 5\b|access is denied|permission denied|拒绝访问/i.test(message)) return L.reportPermissionDenied;
  if (/os error (2|3)\b|not found|cannot find|找不到/i.test(message)) return L.reasonPathMissing;
  return known;
}

/** Containing directory for a failed file in an app-less cleanup; drive roots
 * are excluded so the end-process action never authorizes a whole volume. */
function parentDirOf(path: string): string | undefined {
  const cut = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return cut > 2 ? path.slice(0, cut) : undefined;
}

function ResultGroup({
  label,
  items,
  expanded,
  onRetry,
  onRescan,
  retryBusy,
  lockRoot,
}: {
  label: string;
  items: ItemDetail[];
  expanded: boolean;
  onRetry?: (paths: string[]) => void | Promise<void>;
  onRescan?: () => void;
  retryBusy?: boolean;
  lockRoot?: string;
}) {
  const L = t();
  const [limit, setLimit] = useState(50);
  const [openError, setOpenError] = useState<string | null>(null);
  const [lockInfo, setLockInfo] = useState<Record<string, LockState>>({});
  const diagnose = async (path: string) => {
    try {
      const procs = await api.diagnoseLockedPath(path);
      setLockInfo((prev) => ({ ...prev, [path]: { procs } }));
    } catch (e) {
      setLockInfo((prev) => ({
        ...prev,
        [path]: { procs: [], error: L.lockDiagnoseFailed(backendErrorText(formatError(e))) },
      }));
    }
  };
  const endAndRetry = async (itemPath: string, proc: LockingProcess, root: string | undefined) => {
    if (!root || !proc.path || (!onRetry && !onRescan)) return;
    const ok = await requestConfirmEx({
      title: L.lockEndTitle,
      message: L.lockEndMessage(proc.name, proc.pid),
      confirmLabel: L.lockEndConfirm,
      danger: true,
      holdMs: 600,
    });
    if (!ok.ok) return;
    setLockInfo((prev) => ({
      ...prev,
      [itemPath]: { ...prev[itemPath], procs: prev[itemPath]?.procs ?? [], busyPid: proc.pid },
    }));
    try {
      await api.endLockingProcess(proc.pid, proc.path, itemPath);
    } catch (e) {
      setLockInfo((prev) => ({
        ...prev,
        [itemPath]: {
          procs: prev[itemPath]?.procs ?? [],
          error: L.lockEndFailed(backendErrorText(formatError(e))),
        },
      }));
      return;
    }
    setLockInfo((prev) => {
      const next = { ...prev };
      delete next[itemPath];
      return next;
    });
    try {
      if (onRetry) {
        await onRetry([itemPath]);
      } else {
        onRescan?.();
      }
    } catch (e) {
      setLockInfo((prev) => ({
        ...prev,
        [itemPath]: { procs: [], error: L.lockRetryFailed(backendErrorText(formatError(e))) },
      }));
    }
  };
  const failed = items.filter((item) => item.status === "failed");
  return <details open={expanded} style={{ marginTop: 8 }}>
    <summary>{label} · {items.length}</summary>
    {openError && <p role="alert">{openError}</p>}
    {onRetry && failed.length > 0 && (
      <button
        type="button"
        style={{ ...css.btnGhost, height: 26, fontSize: 12, marginTop: 4 }}
        disabled={retryBusy}
        onClick={() => { void onRetry(failed.map((item) => item.path)); }}
      >
        {L.retryFailedItems} ({failed.length})
      </button>
    )}
    {/* UX-12: the scroll box is reachable by keyboard so a long failure list can
        be paged with the arrow keys instead of only by mouse wheel. */}
    <div tabIndex={0} role="group" aria-label={`${label} · ${items.length}`}
      style={{ maxHeight: "min(320px, 45vh)", overflow: "auto" }}>
      {items.slice(0, limit).map((item, index) => {
        const canLocate = ["file", "dir", "path"].includes(item.kind) && /^[a-z]:[\\/]/i.test(item.path)
          && !item.path.includes('"') && ![...item.path].some(char => char.charCodeAt(0) < 32);
        const lock = lockInfo[item.path];
        // App cleanups authorize the install root; orphan cleanups authorize
        // the reported directory (or the failed file's parent). The backend
        // authorization map remains authoritative.
        const fallbackRoot =
          item.kind === "dir" ? item.path : item.kind === "file" ? parentDirOf(item.path) : undefined;
        const effectiveLockRoot = lockRoot || fallbackRoot;
        return <div key={`${item.path}-${index}`} style={{ padding: "6px 0", borderBottom: "1px solid var(--border)" }}>
          <code style={{ display: "block", whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{item.path}</code>
          {item.message && <div>{reason(item.message)}</div>}
          {item.status === "failed" && <button style={{ ...css.btnGhost, height: 28 }} disabled={!canLocate}
            title={canLocate ? item.path : L.reportLocateUnavailable} aria-label={`${L.openLocation}: ${item.path}`}
            onClick={() => { setOpenError(null); void api.openPath(item.path).catch(e => setOpenError(`${item.path}: ${formatError(e)}`)); }}>{L.openLocation}</button>}
          {item.status === "failed" && (
            <button style={{ ...css.btnGhost, height: 28 }} disabled={!canLocate}
              onClick={() => void diagnose(item.path)}>{L.lockDiagnose}</button>
          )}
          {lock && (
            <div style={css.muted}>
              {lock.error
                ? lock.error
                : lock.procs.length === 0
                  ? L.lockDiagnoseEmpty
                  : lock.procs.map((proc) => (
                      <div key={proc.pid}>
                        {proc.name} ({proc.pid})
                        {proc.path ? <code style={{ marginLeft: 6 }}>{proc.path}</code> : null}
                        {(onRetry || onRescan) && imageUnderRoot(proc.path, effectiveLockRoot) && (
                          <button
                            style={{ ...css.btnGhost, height: 24, marginLeft: 6 }}
                            disabled={lock.busyPid !== undefined}
                            onClick={() => void endAndRetry(item.path, proc, effectiveLockRoot)}
                          >
                            {onRetry ? L.lockEndRetry : L.lockEndRescan}
                          </button>
                        )}
                      </div>
                    ))}
            </div>
          )}
          {item.status === "failed" && !canLocate && <span style={css.muted}>{L.reportLocateUnavailable}</span>}
          {item.message && <details><summary>{L.reportItemDetails}</summary>
            <pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{item.message}</pre></details>}
        </div>;
      })}
    </div>
    {items.length > limit && <button style={css.btnGhost} onClick={() => setLimit(n => n + 50)}>{L.reportShowMore(items.length - limit)}</button>}
  </details>;
}

/**
 * Grouped per-item cleanup outcome. Rendered inline by the software report panel
 * and, since UX-04, by the scoped specialty panels — the toast is gone after a
 * few seconds but a user who cleared 20 caches with 3 failures still needs to
 * know *which* 3, why, and what to do about them.
 */
export function CleanupResultDetails({
  items,
  title,
  onRetry,
  onRescan,
  retryBusy,
  lockRoot,
}: {
  items: ItemDetail[];
  /** Rendered as a heading; omit inside a container that already labels the block. */
  title?: string;
  /** Retry the given failed paths; omit to hide the retry affordance. */
  onRetry?: (paths: string[]) => void | Promise<void>;
  /** Safe fallback after ending a process when the caller cannot retry directly. */
  onRescan?: () => void;
  retryBusy?: boolean;
  /** Install directory that bounds which locking processes may be ended. */
  lockRoot?: string;
}) {
  const L = t();
  const groups = [["failed", L.reportFailed], ["delayed", L.reportPendingReboot], ["skipped", L.reportSkipped],
    ["deleted", L.reportDeleted], ["planned", L.dryRunPlanned]] as const;
  const known = new Set<string>(groups.map(([status]) => status));
  const other = items.filter(item => !known.has(item.status));
  return <div style={{ color: "var(--muted)", fontSize: 12 }}>
    {title && <h3 style={{ margin: "8px 0 0", fontSize: 12.5, fontWeight: 650, color: "var(--fg)" }}>{title}</h3>}
    {groups.map(([status, label]) => {
      const entries = items.filter(item => item.status === status);
      return entries.length ? <ResultGroup key={status} label={label} items={entries} expanded={status === "failed"}
        onRetry={status === "failed" ? onRetry : undefined}
        onRescan={status === "failed" ? onRescan : undefined}
        retryBusy={retryBusy} lockRoot={lockRoot} /> : null;
    })}
    {!!other.length && <ResultGroup label={L.reportOtherResults} items={other} expanded={false} />}
  </div>;
}
