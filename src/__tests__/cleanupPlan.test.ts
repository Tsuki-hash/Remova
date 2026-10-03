import { expect, it } from "vitest";
import { summarizeCleanupPlan } from "../components/CleanupPlanPreview";
import type { CleanupItem } from "../types";

const item = (path: string, kind: CleanupItem["kind"], size_kb?: number | null): CleanupItem =>
  ({ path, kind, size_kb, risk: "low", confidence: "confirmed", score: 90, reason: "", evidence: [] });

it("estimates only selected file roots without double counting children or registry sizes", () => {
  const items = [item("C:/Tool", "dir", 100), item("c:/tool/child", "file", 40),
    item("C:/Toolbox", "file", 5), item("reg", "registry", 999), item("unselected", "file", 999)];
  expect(summarizeCleanupPlan(items, new Set(items.slice(0, 4).map(i => i.path))))
    .toEqual({ count: 4, knownKb: 105, unknown: 0, review: 0, registry: 1, path: 0 });
});

it("keeps zero size distinct from missing or invalid sizes and flags user/shared data", () => {
  const items = [item("zero", "file", 0), item("unknown", "dir"), item("invalid", "file", NaN),
    { ...item("shared", "file", 1), shared: true }, { ...item("user", "file", 2), user_library: true }];
  const plan = summarizeCleanupPlan(items, new Set([...items.map(i => i.path), "stale-selection"]));
  expect(plan).toMatchObject({ count: 5, knownKb: 3, unknown: 2, review: 2 });
});
