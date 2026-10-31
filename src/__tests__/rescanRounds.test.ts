import { describe, expect, it, beforeEach } from "vitest";
import {
  recordReviewRound,
  reviewConvergedFor,
  reviewRoundsFor,
  resetReviewRounds,
} from "../lib/rescanRounds";
import type { CleanupItem } from "../types";

const item = (path: string): CleanupItem => ({
  path, kind: "dir", score: 35, confidence: "suspected", risk: "medium",
  reason: "orphan", evidence: [],
});

describe("rescanRounds", () => {
  beforeEach(resetReviewRounds);

  it("cannot converge from a single recorded round", () => {
    recordReviewRound("app-a", [item("C:\\a")], true);
    expect(reviewRoundsFor("app-a")).toBe(1);
    expect(reviewConvergedFor("app-a")).toBeNull();
  });

  it("converges when two complete rounds agree and keeps keys separate", () => {
    recordReviewRound("app-a", [item("C:\\a"), item("C:\\b")], true);
    recordReviewRound("app-a", [item("C:\\b"), item("C:\\a")], true);
    expect(reviewConvergedFor("app-a")).toBe(true);
    expect(reviewConvergedFor("app-b")).toBeNull();
  });

  it("an incomplete round blocks convergence", () => {
    recordReviewRound("app-a", [item("C:\\a")], true);
    recordReviewRound("app-a", [item("C:\\a")], false);
    expect(reviewConvergedFor("app-a")).toBeNull();
  });

  it("keeps only the two latest rounds per app", () => {
    recordReviewRound("app-a", [item("C:\\old")], true);
    recordReviewRound("app-a", [item("C:\\mid")], true);
    recordReviewRound("app-a", [item("C:\\mid")], true);
    // Latest pair (mid, mid) agrees even though old disappeared.
    expect(reviewConvergedFor("app-a")).toBe(true);
  });
});
