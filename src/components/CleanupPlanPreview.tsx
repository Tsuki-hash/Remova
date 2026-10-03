import type { CleanupItem } from "../types";
import { formatSize, t } from "../i18n";
import { cssStyles as css } from "../styles";

export function summarizeCleanupPlan(items: CleanupItem[], selected: Set<string>) {
  const picked = items.filter(item => selected.has(item.path));
  const key = (path: string) => path.replaceAll("/", "\\").toLowerCase().replace(/\\+$/, "");
  const unique = new Map<string, CleanupItem>();
  for (const item of picked.filter(item => item.kind === "file" || item.kind === "dir")) {
    if (!unique.has(key(item.path))) unique.set(key(item.path), item);
  }
  const directories = new Set([...unique].filter(([, item]) => item.kind === "dir").map(([path]) => path));
  const roots = [...unique].filter(([path]) => {
    for (let end = path.lastIndexOf("\\"); end >= 0; end = path.lastIndexOf("\\", end - 1)) {
      if (directories.has(path.slice(0, end))) return false;
      if (end === 0) break;
    }
    return true;
  });
  let knownKb = 0, unknown = 0;
  for (const [, item] of roots) {
    if (item.size_kb != null && Number.isFinite(item.size_kb) && item.size_kb >= 0) knownKb += item.size_kb;
    else unknown++;
  }
  return { count: picked.length, knownKb, unknown,
    review: picked.filter(item => item.risk !== "low" || item.confidence !== "confirmed"
      || item.shared || item.user_data || item.user_library).length,
    registry: picked.filter(item => item.kind === "registry").length,
    path: picked.filter(item => item.kind === "path").length };
}

export function CleanupPlanPreview({ items, selected }: { items: CleanupItem[]; selected: Set<string> }) {
  const L = t();
  const plan = summarizeCleanupPlan(items, selected);
  return <section aria-label={L.cleanupPlanTitle} style={{ ...css.card, padding: "10px 12px", marginBottom: 8 }}>
    <div style={{ display: "flex", flexWrap: "wrap", gap: "8px 24px", alignItems: "baseline" }}>
      <strong style={{ fontSize: 13 }}>{L.cleanupPlanTitle}</strong>
      <span>{L.itemCount(plan.count)}</span>
      <span>{L.cleanupPlanSize}: <span style={{ fontFamily: "var(--mono)" }}>{formatSize(plan.knownKb)}</span></span>
      {plan.unknown > 0 && <span style={css.muted}>{L.cleanupPlanUnknown(plan.unknown)}</span>}
      <span style={{ color: plan.review ? "var(--warn-ink)" : "var(--muted)" }}>{L.cleanupPlanReview(plan.review)}</span>
    </div>
    <details style={{ marginTop: 6, fontSize: 12, color: "var(--muted)" }}>
      <summary style={{ cursor: "pointer" }}>{L.cleanupPlanDetails}</summary>
      <p style={{ margin: "6px 0" }}>{L.cleanupPlanOther(plan.registry, plan.path)}</p>
      <p style={{ margin: "6px 0" }}>{L.cleanupPlanBackup}</p>
      <p style={{ margin: "6px 0" }}>{L.cleanupPlanEstimate}</p>
    </details>
  </section>;
}
