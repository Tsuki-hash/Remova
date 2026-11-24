// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { DiskRadarComparison } from "../components/DiskRadarComparison";
import { t } from "../i18n";
import { compareDiskRadar, DISK_RADAR_SNAPSHOT_KEY, readDiskRadarSnapshot, snapshotDiskRadar } from "../lib/diskRadarSnapshot";
import type { DirSizeRow } from "../types";

const scope = { volume: "volume-guid", range: "volume:v1" };
const row = (name: string, size_kb: number, capped = false, drive = "D"): DirSizeRow => ({
  name, size_kb, capped, path: `${drive}:\\${name}`, parent: `${drive}:\\`,
});

afterEach(() => { cleanup(); localStorage.clear(); });

it("requires the same volume and scope, and compares only complete matching rows", () => {
  const saved = snapshotDiskRadar(scope.volume, scope.range, [row("Large", 100), row("Floor", 30, true), row("Gone", 20)]);
  const current = [row("Another", 15, false, "E"), row("Large", 180, false, "E"), row("Floor", 40, false, "E")];
  expect(compareDiskRadar(saved, "other-volume", scope.range, current)).toBeNull();
  expect(compareDiskRadar(saved, scope.volume, "directory:v1:\\Nested", current)).toBeNull();
  const changes = compareDiskRadar(saved, scope.volume, scope.range, current)!;
  expect(changes[0]).toMatchObject({ name: "Large", before: 100, after: 180, delta: 80 });
  expect(changes.find(change => change.name === "Floor")?.delta).toBeUndefined();
  expect(changes.find(change => change.name === "Gone")?.after).toBeUndefined();
  expect(changes.find(change => change.name === "Another")?.before).toBeUndefined();
});

it("saves the scan time, keeps the baseline on a new scan, and opens a growing directory", () => {
  const L = t();
  const measuredAt = 1_700_000_000_000;
  const settled = { scope, busy: false, stale: false };
  const first = render(<DiskRadarComparison {...settled} measuredAt={measuredAt} rows={[row("Large", 100), row("Smaller", 40)]} />);
  expect(screen.getByText(L.diskRadarNeedBaseline)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: L.diskRadarSaveSnapshot }));
  const baseline = readDiskRadarSnapshot(localStorage);
  expect(baseline?.takenAt).toBe(measuredAt);
  first.unmount();
  const growing = row("Large", 180);
  const inspect = vi.fn();
  render(<DiskRadarComparison {...settled} measuredAt={measuredAt + 86_400_000}
    rows={[growing, row("Smaller", 15)]} onInspect={inspect} />);
  fireEvent.click(screen.getByText(L.diskRadarCompareDetails(1)));
  const growth = screen.getByRole("list", { name: L.diskRadarLargestGrowth });
  expect(within(growth).getByText("+80 KB")).toBeTruthy();
  expect(within(growth).getByText(L.diskRadarGrowthSizes("100 KB", "180 KB"))).toBeTruthy();
  expect(screen.getByText("−25 KB")).toBeTruthy();
  const inspectGrowth = within(growth).getByRole("button", { name: `${L.diskRadarInspectGrowth}: ${growing.path}` });
  fireEvent.click(inspectGrowth);
  expect(inspect).toHaveBeenCalledWith(growing, inspectGrowth);
  expect(readDiskRadarSnapshot(localStorage)).toEqual(baseline);
  fireEvent.click(screen.getByRole("button", { name: L.diskRadarClearSnapshot }));
  expect(screen.getByRole("button", { name: L.diskRadarSaveSnapshot })).toBeTruthy();
});

it("ranks the five largest measured gains without treating missing or capped rows as growth", () => {
  const L = t();
  const rows = Array.from({ length: 6 }, (_, i) => row(`Folder ${i + 1}`, 100));
  const total = { ...row("Total", 900), aggregate: true };
  const saved = snapshotDiskRadar(scope.volume, scope.range, [...rows, row("Floor", 10, true), row("Missing", 30), total]);
  localStorage.setItem(DISK_RADAR_SNAPSHOT_KEY, JSON.stringify(saved));
  const now = [...rows.map((item, i) => ({ ...item, size_kb: 110 + i })),
    row("Floor", 1000), row("New row", 1000), { ...total, size_kb: 2000 }];
  const { rerender } = render(<DiskRadarComparison scope={scope} rows={now}
    measuredAt={saved.takenAt + 20} busy={false} stale={false} />);
  const largest = screen.getByRole("list", { name: L.diskRadarLargestGrowth });
  expect(within(largest).getAllByRole("listitem")).toHaveLength(5);
  expect(within(largest).getAllByRole("listitem").map(item => item.querySelector("strong")?.textContent))
    .toEqual(["Folder 6", "Folder 5", "Folder 4", "Folder 3", "Folder 2"]);
  expect(screen.getByText(L.diskRadarCappedRow)).toBeTruthy();
  expect(screen.getByText(L.diskRadarNewRow)).toBeTruthy();
  expect(screen.getByText(L.diskRadarMissingRow)).toBeTruthy();
  rerender(<DiskRadarComparison scope={{ ...scope, volume: "different" }} rows={now}
    measuredAt={saved.takenAt + 20} busy={false} stale={false} />);
  expect(screen.queryByRole("list", { name: L.diskRadarLargestGrowth })).toBeNull();
  expect(screen.getByText(L.diskRadarScopeMismatch)).toBeTruthy();
});

it("marks carried-over results and cannot replace the baseline after an unfinished scan", () => {
  const L = t();
  const baseline = snapshotDiskRadar(scope.volume, scope.range, [row("Large", 100)]);
  localStorage.setItem(DISK_RADAR_SNAPSHOT_KEY, JSON.stringify(baseline));
  render(<DiskRadarComparison scope={scope} rows={[row("Large", 170)]}
    measuredAt={baseline.takenAt + 1000} busy={false} stale />);
  expect(screen.getByText((_, node) => Boolean(node?.textContent?.endsWith(L.diskRadarShowingPrevious) && node.tagName === "P"))).toBeTruthy();
  const replace = screen.getByRole<HTMLButtonElement>("button", { name: L.diskRadarReplaceSnapshot });
  expect(replace.disabled).toBe(true);
  expect(screen.getByText("+70 KB")).toBeTruthy();
  expect(readDiskRadarSnapshot(localStorage)).toEqual(baseline);
});
