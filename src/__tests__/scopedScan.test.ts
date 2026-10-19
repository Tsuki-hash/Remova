import { describe, expect, it } from "vitest";
import { groupScannedItems, normalizeScopedScan } from "../lib/scopedScan";
import type { CleanupItem, ScopedScanResult } from "../types";

const item = (path: string, over: Partial<CleanupItem> = {}): CleanupItem => ({
  path, kind: "dir", score: 90, confidence: "confirmed", risk: "low", reason: "tool cache (dev)",
  evidence: [], ...over,
});

describe("normalizeScopedScan", () => {
  it("passes the new backend shape through untouched", () => {
    const raw: ScopedScanResult = { items: [item("a")], omitted: 7, degraded: ["C:\\unreadable"] };
    expect(normalizeScopedScan(raw)).toEqual(raw);
  });
  // The backend fields land in a separate change; a bare array must not crash the
  // panel, and must not be reported as "complete" when it might not be.
  it("tolerates a bare item array while the backend rollout is in flight", () => {
    expect(normalizeScopedScan([item("a")])).toEqual({ items: [item("a")], omitted: 0, degraded: [] });
    expect(normalizeScopedScan(null)).toEqual({ items: [], omitted: 0, degraded: [] });
    expect(normalizeScopedScan(undefined)).toEqual({ items: [], omitted: 0, degraded: [] });
  });
});

describe("groupScannedItems", () => {
  it("groups by bucket and sorts by measured size descending inside each group", () => {
    const groups = groupScannedItems([
      item("small-dev", { bucket: "dev", size_kb: 4_096 }),
      item("big-dev", { bucket: "dev", size_kb: 5_242_880 }),
      item("browser", { bucket: "browser", size_kb: 2_097_152 }),
      item("tiny-dev", { bucket: "dev", size_kb: 512 }),
    ]);
    expect(groups.map(g => g.bucket)).toEqual(["browser", "dev"]);
    expect(groups[1]!.items.map(i => i.path)).toEqual(["big-dev", "small-dev", "tiny-dev"]);
  });

  it("sinks unmeasured sizes to the bottom instead of treating them as zero", () => {
    const groups = groupScannedItems([
      item("unknown", { bucket: "dev", size_kb: null }),
      item("measured", { bucket: "dev", size_kb: 1 }),
      item("omitted-field", { bucket: "dev" }),
    ]);
    // Both `null` and an absent field mean "not measured", and neither may be
    // read as 0 KB and floated to the top of the group.
    expect(groups[0]!.items.map(i => i.path)).toEqual(["measured", "omitted-field", "unknown"]);
  });

  it("keeps a stable order for equal sizes and a named header for a missing bucket", () => {
    const groups = groupScannedItems([
      item("b", { bucket: "dev", size_kb: 10 }),
      item("a", { bucket: "dev", size_kb: 10 }),
      item("no-bucket", { size_kb: 10 }),
    ]);
    expect(groups[0]!.items.map(i => i.path)).toEqual(["a", "b"]);
    // Unknown bucket sorts last but still forms its own group.
    expect(groups.map(g => g.bucket)).toEqual(["dev", ""]);
    expect(groups[1]!.items.map(i => i.path)).toEqual(["no-bucket"]);
  });
});
