// @vitest-environment jsdom
// Disk radar: a budget-capped row must present itself as an honest floor,
// a root aggregate must be labelled as a total, and a missing entry count must
// not be rendered as a definite zero.
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
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
    // The radar reads the raw result (the drill-down carries omitted/capped),
    // so the mock must run the work like the real hook does.
    runReadOnly: (
      fn: (id: number) => Promise<unknown>,
      _count?: (result: unknown) => number,
    ) => fn(1),
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

  it("does not invent a zero entry count when the backend sends none", async () => {
    mockDrives();
    vi.mocked(api.listTopDirSizes).mockResolvedValue([
      {
        path: "C:\\Windows",
        name: "Windows",
        size_kb: 5_000,
        parent: "C:\\",
        capped: true,
      },
    ]);
    render(<DiskRadarPanel onClose={vi.fn()} onError={vi.fn()} />);
    const L = t();
    const rows = await screen.findAllByText((_content, element) =>
      (element?.textContent ?? "").includes(L.diskRadarFloor(formatSize(5_000))),
    );
    expect(rows.length).toBeGreaterThan(0);
    // The unknown-count wording must not claim a count at all. Compare against
    // the i18n text (not a Chinese literal) so the pin survives a language
    // switch and fails when the implementation starts printing "0".
    const counted = L.diskRadarFloor(formatSize(5_000), 0);
    expect(rows[0]!.textContent).not.toContain(counted);
  });

  it("labels a root aggregate row as a total", async () => {
    mockDrives();
    vi.mocked(api.listTopDirSizes).mockResolvedValue([
      {
        path: "C:\\Users",
        name: "Users",
        size_kb: 44_000_000,
        parent: "C:\\",
        capped: true,
        scanned: 900_000,
        aggregate: true,
      },
      {
        path: "C:\\Users\\Aaron",
        name: "Aaron",
        size_kb: 40_000_000,
        parent: "C:\\Users",
        capped: false,
      },
    ]);
    render(<DiskRadarPanel onClose={vi.fn()} onError={vi.fn()} />);
    const L = t();
    // The explanation is real text, not a title: keyboard and screen-reader
    // users reach it through aria-describedby on the badge around it.
    const described = await screen.findByText(L.diskRadarAggregateHint);
    const badge = described.parentElement!;
    expect(badge.getAttribute("aria-describedby")).toBe(described.id);
    // The id must survive an IDREF split: paths contain spaces
    // ("C:\\Program Files") and a space would break the association.
    expect(described.id).not.toMatch(/\s/);
    // Hover keeps the explanation for pointer users.
    expect(badge.getAttribute("title")).toBe(L.diskRadarAggregateHint);
    expect(badge.textContent).toContain(L.diskRadarAggregate);
    // One badge only: the child row is a single directory, not a total.
    expect(screen.getAllByText(L.diskRadarAggregateHint)).toHaveLength(1);
  });

  it("tells the user when a drill-down list is partial", async () => {
    mockDrives();
    vi.mocked(api.listTopDirSizes).mockResolvedValue([
      { path: "C:\\Users", name: "Users", size_kb: 100, parent: "C:\\", capped: false },
    ]);
    vi.mocked(api.listDirChildren).mockResolvedValue({
      rows: [
        { path: "C:\\Users\\A", name: "A", size_kb: 10, parent: "C:\\Users", capped: false },
      ],
      omitted: 5,
      capped: true,
    });
    render(<DiskRadarPanel onClose={vi.fn()} onError={vi.fn()} />);
    const L = t();
    fireEvent.click(await screen.findByRole("button", { name: L.detailShow }));
    // A list that leaves rows out must say so; the user cannot otherwise tell
    // a complete listing from a capped one.
    expect(await screen.findByText(L.scanOmitted(5))).toBeTruthy();
    expect(screen.getByText(L.diskRadarPartial)).toBeTruthy();
  });

  it("says a capped drill-down is incomplete instead of calling it empty", async () => {
    mockDrives();
    vi.mocked(api.listTopDirSizes).mockResolvedValue([
      { path: "C:\\Users", name: "Users", size_kb: 100, parent: "C:\\", capped: false },
    ]);
    vi.mocked(api.listDirChildren).mockResolvedValue({
      rows: [],
      omitted: 3,
      capped: true,
    });
    render(<DiskRadarPanel onClose={vi.fn()} onError={vi.fn()} />);
    const L = t();
    fireEvent.click(await screen.findByRole("button", { name: L.detailShow }));
    // An empty list that was cut short must not read as "nothing here".
    expect(await screen.findByText(L.diskRadarPartial)).toBeTruthy();
    expect(screen.queryByText(L.radarEmpty)).toBeNull();
    // The count is hidden on an empty list; the note above carries the news.
    expect(screen.queryByText(L.scanOmitted(3))).toBeNull();
  });

  it("labels a drilled-down empty folder as such, not as an empty drive", async () => {
    mockDrives();
    vi.mocked(api.listTopDirSizes).mockResolvedValue([
      { path: "C:\\Users", name: "Users", size_kb: 100, parent: "C:\\", capped: false },
    ]);
    vi.mocked(api.listDirChildren).mockResolvedValue({ rows: [], omitted: 0, capped: false });
    render(<DiskRadarPanel onClose={vi.fn()} onError={vi.fn()} />);
    const L = t();
    fireEvent.click(await screen.findByRole("button", { name: L.detailShow }));
    expect(await screen.findByText(L.diskRadarEmptyFolder)).toBeTruthy();
    expect(screen.queryByText(L.radarEmpty)).toBeNull();
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
