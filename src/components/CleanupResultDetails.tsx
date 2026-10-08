import { useState } from "react";
import { api } from "../lib/api";
import { formatError } from "../lib/format";
import { gateReasonText } from "../lib/decision";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";
import type { ItemDetail } from "../types";

function reason(message: string) {
  const L = t();
  const known = gateReasonText(message, L);
  if (known !== L.backendDetailUnavailable) return known;
  if (/os error (32|33)\b|sharing violation|being used by another process|另一个进程|另一进程/i.test(message)) return L.reportFileBusy;
  if (/os error 5\b|access is denied|permission denied|拒绝访问/i.test(message)) return L.reportPermissionDenied;
  if (/os error (2|3)\b|not found|cannot find|找不到/i.test(message)) return L.reasonPathMissing;
  return known;
}

function ResultGroup({ label, items, expanded }: { label: string; items: ItemDetail[]; expanded: boolean }) {
  const L = t();
  const [limit, setLimit] = useState(50);
  const [openError, setOpenError] = useState<string | null>(null);
  return <details open={expanded} style={{ marginTop: 8 }}>
    <summary>{label} · {items.length}</summary>
    {openError && <p role="alert">{openError}</p>}
    <div style={{ maxHeight: 180, overflow: "auto" }}>
      {items.slice(0, limit).map((item, index) => {
        const canLocate = ["file", "dir", "path"].includes(item.kind) && /^[a-z]:[\\/]/i.test(item.path)
          && !item.path.includes('"') && ![...item.path].some(char => char.charCodeAt(0) < 32);
        return <div key={`${item.path}-${index}`} style={{ padding: "6px 0", borderBottom: "1px solid var(--border)" }}>
          <code style={{ display: "block", whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{item.path}</code>
          {item.message && <div>{reason(item.message)}</div>}
          {item.status === "failed" && <button style={{ ...css.btnGhost, height: 28 }} disabled={!canLocate}
            title={canLocate ? item.path : L.reportLocateUnavailable} aria-label={`${L.openLocation}: ${item.path}`}
            onClick={() => { setOpenError(null); void api.openPath(item.path).catch(e => setOpenError(`${item.path}: ${formatError(e)}`)); }}>{L.openLocation}</button>}
          {item.status === "failed" && !canLocate && <span style={css.muted}>{L.reportLocateUnavailable}</span>}
          {item.message && <details><summary>{L.reportItemDetails}</summary>
            <pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{item.message}</pre></details>}
        </div>;
      })}
    </div>
    {items.length > limit && <button style={css.btnGhost} onClick={() => setLimit(n => n + 50)}>{L.reportShowMore(items.length - limit)}</button>}
  </details>;
}

export function CleanupResultDetails({ items }: { items: ItemDetail[] }) {
  const L = t();
  const groups = [["failed", L.reportFailed], ["delayed", L.reportPendingReboot], ["skipped", L.reportSkipped],
    ["deleted", L.reportDeleted], ["planned", L.dryRunPlanned]] as const;
  const known = new Set<string>(groups.map(([status]) => status));
  const other = items.filter(item => !known.has(item.status));
  return <div style={{ color: "var(--muted)", fontSize: 12 }}>
    {groups.map(([status, label]) => {
      const entries = items.filter(item => item.status === status);
      return entries.length ? <ResultGroup key={status} label={label} items={entries} expanded={status === "failed"} /> : null;
    })}
    {!!other.length && <ResultGroup label={L.reportOtherResults} items={other} expanded={false} />}
  </div>;
}
