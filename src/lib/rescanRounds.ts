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

/** The cleanup session each app's rounds belong to. */
const sessionByApp = new Map<string, string>();

/**
 * Bind the app to a cleanup session. Rounds recorded under a different session
 * are dropped: a new cleanup invalidates the previous convergence verdict,
 * which otherwise kept exporting `true` for a run nobody had reviewed.
 */
export function beginReviewSession(appKey: string, sessionId: string): void {
  const key = appKey.trim() || "(unknown)";
  if (sessionByApp.get(key) === sessionId) return;
  sessionByApp.set(key, sessionId);
  roundsByApp.delete(key);
}

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

/** Why a convergence verdict is unavailable, when it is. */
export type ReviewVerdict =
  | { state: "converged" }
  | { state: "diverged" }
  | { state: "insufficient_rounds"; rounds: number }
  | { state: "incomplete_round" };

/**
 * Convergence verdict with the reason it is unavailable. "Not enough rounds"
 * and "a round failed or was cancelled" are different situations and must not
 * collapse into one "fewer than two rounds" message.
 */
export function reviewVerdictFor(appKey: string): ReviewVerdict {
  const rounds = roundsByApp.get(appKey.trim()) ?? [];
  if (rounds.some((r) => !r.complete)) return { state: "incomplete_round" };
  if (rounds.length < 2) return { state: "insufficient_rounds", rounds: rounds.length };
  const [a, b] = rounds.slice(-2);
  const ka = new Set(a!.items.map(residueKey));
  const kb = new Set(b!.items.map(residueKey));
  if (ka.size !== kb.size) return { state: "diverged" };
  for (const key of ka) {
    if (!kb.has(key)) return { state: "diverged" };
  }
  return { state: "converged" };
}

export function reviewConvergedFor(appKey: string): boolean | null {
  const verdict = reviewVerdictFor(appKey);
  if (verdict.state === "converged") return true;
  if (verdict.state === "diverged") return false;
  return null;
}

/** Test hook: clear every recorded round. */
export function resetReviewRounds(): void {
  roundsByApp.clear();
  sessionByApp.clear();
}
