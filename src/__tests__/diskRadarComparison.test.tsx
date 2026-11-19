// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { DiskRadarComparison } from "../components/DiskRadarComparison";
import { t } from "../i18n";
import { compareDiskRadar, snapshotDiskRadar } from "../lib/diskRadarSnapshot";
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

it("can save, reopen, inspect a growing directory and clear the saved measurement", () => {
  const L = t();
  const first = render(<DiskRadarComparison scope={scope} rows={[row("Large", 100), row("Smaller", 40)]} busy={false} />);
  fireEvent.click(screen.getByRole("button", { name: L.diskRadarSaveSnapshot }));
  first.unmount();
  const growing = row("Large", 180);
  const inspect = vi.fn();
  render(<DiskRadarComparison scope={scope} rows={[growing, row("Smaller", 15)]} busy={false} onInspect={inspect} />);
  fireEvent.click(screen.getByText(L.diskRadarCompareDetails(2)));
  expect(screen.getByText("+80 KB")).toBeTruthy();
  expect(screen.getByText("−25 KB")).toBeTruthy();
  const inspectGrowth = within(screen.getByRole("region", { name: L.diskRadarCompareTitle })).getByRole("button", { name: L.folderInspectAction });
  fireEvent.click(inspectGrowth);
  expect(inspect).toHaveBeenCalledWith(growing, inspectGrowth);
  fireEvent.click(screen.getByRole("button", { name: L.diskRadarClearSnapshot }));
  expect(screen.getByRole("button", { name: L.diskRadarSaveSnapshot })).toBeTruthy();
});
