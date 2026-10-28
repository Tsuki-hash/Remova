// @vitest-environment jsdom
// Disk radar: a budget-capped row must present itself as an honest floor
// ("stopped after N entries"), not as a total.
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { DiskRadarPanel } from "../components/DiskRadarPanel";
import { api } from "../lib/api";
import { t, formatSize } from "../i18n";

vi.mock("../lib/api", () => ({
  api: {
    listLocalDrives: vi.fn(),
    listTopDirSizes: vi.fn(),
    listDirChildren: vi.fn(),
    openPath: vi.fn(),
  },
}));
vi.mock("../hooks/useAssociationScan", () => ({
  useAssociationScan: () => ({
    run: vi.fn(),
    runItems: (fn: (id: number) => Promise<unknown>) => fn(1),
    runReadOnly: vi.fn(),
    cancel: vi.fn(),
    progress: null,
  }),
  scanCancelled: () => false,
}));

afterEach(() => {
  cleanup();
  for (const mock of [
    api.listLocalDrives,
    api.listTopDirSizes,
    api.listDirChildren,
    api.openPath,
  ]) {
    vi.mocked(mock).mockReset();
  }
  vi.clearAllMocks();
});

function mockDrives() {
  vi.mocked(api.listLocalDrives).mockResolvedValue([
    { letter: "C", free_gb: 58.3, total_gb: 214.75, is_system: true },
  ]);
}

describe("DiskRadarPanel", () => {
  it("shows a capped row as a floor with the entry count", async () => {
    mockDrives();
    vi.mocked(api.listTopDirSizes).mockResolvedValue([
      {
        path: "C:\\Users\\Aaron",
        name: "Aaron",
        size_kb: 20_000_000,
        parent: "C:\\Users",
        capped: true,
        scanned: 12_345,
      },
      {
        path: "C:\\Windows\\Installer",
        name: "Installer",
        size_kb: 19_490_000,
        parent: "C:\\Windows",
        capped: false,
      },
    ]);
    render(<DiskRadarPanel onClose={vi.fn()} onError={vi.fn()} />);
    const L = t();
    // The matcher matches ancestors too, so assert "at least the row exists".
    const floors = await screen.findAllByText((_content, element) =>
      (element?.textContent ?? "").includes(
        L.diskRadarFloor(formatSize(20_000_000), 12345),
      ),
    );
    expect(floors.length).toBeGreaterThan(0);
    // A completed walk keeps the "approx" wording — the two modes must stay
    // distinguishable.
    const approx = await screen.findAllByText((_content, element) =>
      (element?.textContent ?? "").includes(L.diskSizeApprox(formatSize(19_490_000))),
    );
    expect(approx.length).toBeGreaterThan(0);
  });

  it("reports a load failure through onError instead of an empty list", async () => {
    mockDrives();
    vi.mocked(api.listTopDirSizes).mockRejectedValue(new Error("radar boom"));
    const onError = vi.fn();
    render(<DiskRadarPanel onClose={vi.fn()} onError={onError} />);
    await vi.waitFor(() => expect(onError).toHaveBeenCalled());
    expect(vi.mocked(onError).mock.calls[0]![0]).toContain("radar boom");
  });
});
