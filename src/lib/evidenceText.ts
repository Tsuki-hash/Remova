import type { Evidence } from "../types";
import { t } from "../i18n";

/** Native evidence codes are stable; paths/extensions remain technical data. */
export function evidenceText(evidence: Evidence): Pick<Evidence, "label" | "detail"> {
  const L = t();
  const labels: Record<string, string> = { orphan_no_owner: L.orphanEvNoOwner,
    orphan_has_exe: L.orphanEvExe, orphan_many_files: L.orphanEvFiles,
    orphan_has_config: L.orphanEvConfig, orphan_root: L.orphanEvRoot, orphan_mtime: L.orphanEvAge };
  if (!Object.hasOwn(labels, evidence.code)) {
    const label = L.scanEvidenceLabel(evidence.code, evidence.label);
    return label ? { label, detail: evidence.detail } : evidence;
  }
  let detail = "";
  if (evidence.code === "orphan_has_exe") detail = ".exe / .msi";
  if (evidence.code === "orphan_has_config") detail = ".dll / .dat / .db / .json / .ini / .xml";
  if (evidence.code === "orphan_root") detail = evidence.detail;
  const files = evidence.detail.match(/^(\d+) files in top level$/);
  if (evidence.code === "orphan_many_files" && files) detail = L.orphanEvFileCount(Number(files[1]));
  const days = evidence.detail.match(/^(\d+) day\(s\) ago$/);
  if (evidence.code === "orphan_mtime" && days) detail = L.orphanEvDays(Number(days[1]));
  return { label: labels[evidence.code]!, detail };
}
