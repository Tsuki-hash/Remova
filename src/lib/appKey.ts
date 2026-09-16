/** Stable row/multi-select key (CODE-8: avoid ambiguous string concat). */
import type { InstalledApp } from "../types";

const SEP = String.fromCharCode(0);

export function appKey(a: InstalledApp): string {
  return [a.source, a.registry_key, a.name].join(SEP);
}
