import type { ScanCompleteness } from "../types";
import { t } from "../i18n";
import { cssStyles as css } from "../styles";

export function ScanCompletenessNotice({ completeness }: { completeness: ScanCompleteness }) {
  if (completeness.status === "complete") return null;
  const L = t();
  const roots = completeness.failed_roots;
  const detail = roots.length ? (
    <ul style={{ margin: "4px 0 0", paddingLeft: 18 }}>
      {roots.map((root, i) => <li key={`${root}-${i}`}>{root}</li>)}
    </ul>
  ) : null;
  const text = completeness.status === "cancelled"
    ? L.scanCompletenessCancelled
    : completeness.status === "failed"
      ? L.scanCompletenessFailed
      : completeness.status === "unknown"
        ? L.scanCompletenessUnknown
        : L.scanCompletenessPartial;
  return (
    // role="note": panels already carry role="status" live regions (progress,
    // scan state) and a second one would break their queries.
    <div role="note" style={{
      ...css.muted,
      fontSize: 12,
      margin: "6px 0",
      padding: "6px 8px",
      borderLeft: "3px solid var(--warn)",
      background: "var(--surface-2)",
    }}>
      <div>{text}</div>
      {completeness.roots_scanned > 0 && (
        <div>{L.scanCompletenessRoots(completeness.roots_scanned)}</div>
      )}
      {detail}
    </div>
  );
}
