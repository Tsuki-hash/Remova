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

it("separates 'not enough rounds' from 'a round failed'", async () => {
  const mod = await import("../lib/rescanRounds");
  mod.resetReviewRounds();
  expect(mod.reviewVerdictFor("app").state).toBe("insufficient_rounds");
  const item = { path: "C:\\a", kind: "dir" as const, score: 1, confidence: "confirmed" as const,
    risk: "low" as const, reason: "r", evidence: [], shared: false, service_bound: false,
    user_data: false, user_library: false };
  mod.recordReviewRound("app", [item], true);
  mod.recordReviewRound("app", [item], false);
  expect(mod.reviewVerdictFor("app").state).toBe("incomplete_round");
  expect(mod.reviewConvergedFor("app")).toBeNull();
  mod.resetReviewRounds();
});

it("a new cleanup session drops the previous convergence verdict", async () => {
  const mod = await import("../lib/rescanRounds");
  mod.resetReviewRounds();
  const item = { path: "C:\\a", kind: "dir" as const, score: 1, confidence: "confirmed" as const,
    risk: "low" as const, reason: "r", evidence: [], shared: false, service_bound: false,
    user_data: false, user_library: false };
  mod.beginReviewSession("app", "cleanup-1");
  mod.recordReviewRound("app", [item], true);
  mod.recordReviewRound("app", [item], true);
  expect(mod.reviewVerdictFor("app").state).toBe("converged");
  // A new cleanup starts over: the old two-round verdict must not survive.
  mod.beginReviewSession("app", "cleanup-2");
  expect(mod.reviewVerdictFor("app").state).toBe("insufficient_rounds");
  mod.recordReviewRound("app", [item], true);
  expect(mod.reviewVerdictFor("app").state).toBe("insufficient_rounds");
  mod.resetReviewRounds();
});

it("startReviewSession clears the verdict a previous cleanup produced", async () => {
  const mod = await import("../lib/rescanRounds");
  mod.resetReviewRounds();
  const item = { path: "C:\\\\a", kind: "dir" as const, score: 1, confidence: "confirmed" as const,
    risk: "low" as const, reason: "r", evidence: [], shared: false, service_bound: false,
    user_data: false, user_library: false };
  mod.startReviewSession("app");
  mod.recordReviewRound("app", [item], true);
  mod.recordReviewRound("app", [item], true);
  expect(mod.reviewVerdictFor("app").state).toBe("converged");
  // A new cleanup run starts over even though the rounds looked converged.
  mod.startReviewSession("app");
  expect(mod.reviewVerdictFor("app").state).toBe("insufficient_rounds");
  mod.resetReviewRounds();
});
