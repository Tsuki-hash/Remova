import { describe, expect, it } from "vitest";
import { recordReviewRound, reviewSessionFor, reviewVerdictFor, startReportReview } from "../lib/rescanRounds";
import type { CleanupItem, FullCleanupReport } from "../types";

const report = (): FullCleanupReport => ({
  app_name: "Demo", app_key: "fixture", dry_run: false, backup_dir: "", uninstall_ok: true,
  uninstall_message: "", deleted: 1, failed: 0, skipped: 0, delayed: 0, aborted: false,
  restore_point_ok: false, restore_point_msg: "", errors: [], item_details: [],
});

const item = (path: string): CleanupItem => ({
  path, kind: "dir", score: 35, confidence: "suspected", risk: "medium",
  reason: "orphan", evidence: [],
});

describe("rescanRounds", () => {
  it("requires two complete rounds and compares the latest pair", () => {
    const cleanup = report();
    startReportReview(cleanup);
    const session = reviewSessionFor(cleanup)!;
    recordReviewRound(session, { items: [item("old")], complete: true });
    expect(reviewVerdictFor(session)).toEqual({ state: "insufficient_rounds", rounds: 1 });
    recordReviewRound(session, { items: [item("new"), item("keep")], complete: true });
    expect(reviewVerdictFor(session)).toEqual({ state: "diverged" });
    recordReviewRound(session, { items: [item("keep"), item("new")], complete: true });
    expect(reviewVerdictFor(session)).toEqual({ state: "converged" });
  });

  it("distinguishes incomplete evidence and keeps only the last three attempts", () => {
    const cleanup = report();
    startReportReview(cleanup);
    const session = reviewSessionFor(cleanup)!;
    recordReviewRound(session, { items: [], complete: false });
    for (let i = 0; i < 2; i++) recordReviewRound(session, { items: [], complete: true });
    expect(reviewVerdictFor(session)).toEqual({ state: "incomplete_round" });
    recordReviewRound(session, { items: [], complete: true });
    expect(session.rounds).toHaveLength(3);
    expect(reviewVerdictFor(session)).toEqual({ state: "converged" });
  });

  it("a new report for the same app does not use old or late evidence", () => {
    const previous = report();
    startReportReview(previous);
    const oldSession = reviewSessionFor(previous)!;
    const current = report();
    startReportReview(current);
    const newSession = reviewSessionFor(current)!;
    recordReviewRound(oldSession, { items: [], complete: true });
    recordReviewRound(oldSession, { items: [], complete: true });

    expect(previous.app_key).toBe(current.app_key);
    expect(reviewVerdictFor(oldSession)).toEqual({ state: "converged" });
    expect(reviewVerdictFor(newSession)).toEqual({ state: "insufficient_rounds", rounds: 0 });
    expect(reviewVerdictFor(reviewSessionFor(report()))).toEqual({ state: "insufficient_rounds", rounds: 0 });
  });
});
