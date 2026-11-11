import type { CleanupReport, FullCleanupReport } from "../types";
import { compareConvergence, type ConvergenceRound } from "./residueConvergence";

const MAX_ROUNDS = 3;
export type ReviewSession = { readonly rounds: ConvergenceRound[] };
const reviewByReport = new WeakMap<FullCleanupReport, ReviewSession>();

/** Each cleanup report owns its evidence even when a later run has the same app key. */
export function startReportReview(report: FullCleanupReport): void {
  reviewByReport.set(report, { rounds: [] });
}

export function reviewSessionFor(report: CleanupReport | FullCleanupReport): ReviewSession | undefined {
  return "deleted" in report ? reviewByReport.get(report) : undefined;
}

export function recordReviewRound(session: ReviewSession, round: ConvergenceRound): void {
  session.rounds.push(round);
  if (session.rounds.length > MAX_ROUNDS) session.rounds.shift();
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
export function reviewVerdictFor(session: ReviewSession | undefined): ReviewVerdict {
  const { rounds, converged, incompleteRounds } = compareConvergence(session?.rounds ?? []);
  if (incompleteRounds) return { state: "incomplete_round" };
  if (converged === null) return { state: "insufficient_rounds", rounds };
  return { state: converged ? "converged" : "diverged" };
}
