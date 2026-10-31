import type { CleanupItem } from "../types";
import { residueKey, type ConvergenceRound } from "./residueConvergence";

/**
 * Review-round storage for the post-uninstall rescan (M4E.1): the two latest
 * rounds per app are kept so the report panel can state whether the residue
 * review converged. Recording happens in `useAnalyzeFlow` after each
 * post-uninstall rescan (`fromUninstall`), with `complete=false` marking a
 * scan that failed or was cancelled — an incomplete round can never converge.
 */

const MAX_ROUNDS = 3;
const roundsByApp = new Map<string, ConvergenceRound[]>();

export function recordReviewRound(
  appKey: string,
  items: CleanupItem[],
  complete: boolean,
): void {
  const key = appKey.trim() || "(unknown)";
  const rounds = roundsByApp.get(key) ?? [];
  rounds.push({ items, complete });
  roundsByApp.set(key, rounds.slice(-MAX_ROUNDS));
}

export function reviewRoundsFor(appKey: string): number {
  return (roundsByApp.get(appKey.trim()) ?? []).length;
}

export function reviewConvergedFor(appKey: string): boolean | null {
  const rounds = roundsByApp.get(appKey.trim()) ?? [];
  if (rounds.length < 2 || rounds.some((r) => !r.complete)) return null;
  const [a, b] = rounds.slice(-2);
  const ka = new Set(a!.items.map(residueKey));
  const kb = new Set(b!.items.map(residueKey));
  if (ka.size !== kb.size) return false;
  for (const key of ka) {
    if (!kb.has(key)) return false;
  }
  return true;
}

/** Test hook: clear every recorded round. */
export function resetReviewRounds(): void {
  roundsByApp.clear();
}
