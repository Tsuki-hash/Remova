import { describe, expect, it } from "vitest";
import { compareConvergence, residueKey } from "../lib/residueConvergence";
import type { CleanupItem } from "../types";

const item = (path: string, over: Partial<CleanupItem> = {}): CleanupItem => ({
  path, kind: "dir", score: 35, confidence: "suspected", risk: "medium",
  reason: "orphan", evidence: [], ...over,
});

const round = (paths: string[], complete = true) => ({
  items: paths.map((p) => item(p)),
  complete,
});

describe("residueKey", () => {
  it("separates the same path across kinds", () => {
    expect(residueKey(item("C:\\a"))).toBe("dir:C:\\a");
    expect(residueKey(item("C:\\a", { kind: "registry" }))).toBe("registry:C:\\a");
  });
});

describe("compareConvergence", () => {
  it("cannot converge from a single round", () => {
    const r = compareConvergence([round(["C:\\a"])]);
    expect(r.converged).toBeNull();
    expect(r.rounds).toBe(1);
    expect(r.remaining).toEqual(["dir:C:\\a"]);
  });

  it("converges when two complete rounds agree", () => {
    const r = compareConvergence([round(["C:\\a", "C:\\b"]), round(["C:\\b", "C:\\a"])]);
    expect(r.converged).toBe(true);
    expect(r.appeared).toEqual([]);
    expect(r.disappeared).toEqual([]);
    expect(r.remaining).toEqual(["dir:C:\\a", "dir:C:\\b"]);
  });

  it("stays open when the sets differ, naming appeared and disappeared", () => {
    const r = compareConvergence([round(["C:\\a", "C:\\old"]), round(["C:\\a", "C:\\new"])]);
    expect(r.converged).toBe(false);
    expect(r.appeared).toEqual(["dir:C:\\new"]);
    expect(r.disappeared).toEqual(["dir:C:\\old"]);
  });

  it("an incomplete round blocks convergence even when sets agree", () => {
    const r = compareConvergence([round(["C:\\a"]), round(["C:\\a"], false)]);
    expect(r.converged).toBeNull();
    expect(r.incompleteRounds).toBe(1);
  });

  it("remaining comes from the latest round even with an incomplete history", () => {
    const r = compareConvergence([round(["C:\\a"]), round(["C:\\b"], false)]);
    expect(r.converged).toBeNull();
    expect(r.remaining).toEqual(["dir:C:\\b"]);
  });
});
