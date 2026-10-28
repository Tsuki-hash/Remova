// @vitest-environment jsdom
// Disk radar: a budget-capped row must present itself as an honest floor,
// a root aggregate must be labelled as a total, and a missing entry count must
// not be rendered as a definite zero.
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { DiskRadarPanel } from "../components/DiskRadarPanel";
import { ScanProgressBar } from "../components/ScanProgressBar";
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
const hookState = vi.hoisted(() => ({
  progress: null as { status: string; stage: string; found: number; elapsedMs: number } | null,
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
    progress: hookState.progress,
  }),
  scanCancelled: () => false,
}));

afterEach(() => {
  hookState.progress = null;
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
        // A root whose path contains a space: the id must stay free of
        // whitespace or the IDREF splits and the description is lost.
        path: "C:\\Program Files (x86)",
        name: "Program Files",
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
    // The explanation is a sibling span; the badge references it by
    // aria-describedby (index-based id, no whitespace even for
    // "C:\\Program Files (x86)").
    const badge = document.querySelector(`[aria-describedby="${described.id}"]`)!;
    expect(badge.getAttribute("aria-describedby")).toBe(described.id);
    expect(described.id).not.toMatch(/\s/);
    // The accessible name is the visible label; the description is external.
    expect(badge.textContent).toBe(L.diskRadarAggregate);
    expect(badge.getAttribute("title")).toBeNull();
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

  it("uses the radar wording for the progress counter and the default elsewhere", async () => {
    const L = t();
    const running = {
      status: "running" as const,
      stage: "sizes",
      found: 4096,
      elapsedMs: 1200,
    };
    const { unmount } = render(
      <ScanProgressBar progress={running} onCancel={async () => {}} foundLabel={L.scanFoundRadar} />,
    );
    expect(screen.getByText(L.scanFoundRadar(4096))).toBeTruthy();
    expect(screen.queryByText(L.scanFound(4096))).toBeNull();
    unmount();
    // Without the prop the shared wording stays untouched.
    render(<ScanProgressBar progress={running} onCancel={async () => {}} />);
    expect(screen.getByText(L.scanFound(4096))).toBeTruthy();
  });

  it("wires the radar counter wording through the panel, not just the bar", async () => {
    mockDrives();
    vi.mocked(api.listTopDirSizes).mockImplementation(
      () => new Promise(() => { /* the scan runs until the test ends */ }),
    );
    // The panel must pass its own label down: removing the prop would
    // otherwise silently fall back to the cleanup scan's "items found".
    hookState.progress = { status: "running", stage: "sizes", found: 4096, elapsedMs: 1200 };
    render(<DiskRadarPanel onClose={vi.fn()} onError={vi.fn()} />);
    const L = t();
    expect(await screen.findByText(L.scanFoundRadar(4096))).toBeTruthy();
    expect(screen.queryByText(L.scanFound(4096))).toBeNull();
  });

  it("shows a zero entry count when the walk proved zero entries", async () => {
    mockDrives();
    vi.mocked(api.listTopDirSizes).mockResolvedValue([
      { path: "C:\\Empty", name: "Empty", size_kb: 0, parent: "C:\\", capped: true, scanned: 0 },
    ]);
    render(<DiskRadarPanel onClose={vi.fn()} onError={vi.fn()} />);
    const L = t();
    // scanned 0 is a real observation, not a missing count: the floor must
    // say "checked 0 entries" instead of hiding the count.
    const rows = await screen.findAllByText((_c, el) =>
      (el?.textContent ?? "").includes(L.diskRadarFloor("0 KB", 0)));
    expect(rows.length).toBeGreaterThan(0);
  });

  it("tells the system-drive overview that only five known roots are counted", async () => {
    mockDrives();
    vi.mocked(api.listTopDirSizes).mockResolvedValue([
      { path: "C:\\Users", name: "Users", size_kb: 100, parent: "C:\\", capped: false },
    ]);
    render(<DiskRadarPanel onClose={vi.fn()} onError={vi.fn()} />);
    const L = t();
    // The five known roots leave a gap against the drive's used space; the
    // panel must say so instead of letting the totals read as "everything".
    expect(await screen.findByText(L.diskRadarRootsHint)).toBeTruthy();
    // Drilling into a folder leaves the five-roots scope behind.
    vi.mocked(api.listDirChildren).mockResolvedValue({ rows: [], omitted: 0, capped: false });
    fireEvent.click(screen.getByRole("button", { name: L.detailShow }));
    await screen.findByText(L.diskRadarEmptyFolder);
    expect(screen.queryByText(L.diskRadarRootsHint)).toBeNull();
  });

  it("shows drive capacity in the same 1024-based units as the rows", async () => {
    mockDrives();
    vi.mocked(api.listTopDirSizes).mockResolvedValue([]);
    render(<DiskRadarPanel onClose={vi.fn()} onError={vi.fn()} />);
    const L = t();
    await screen.findByText(L.radarEmpty);
    // free_gb/total_gb are decimal GB while rows are 1024-based; the capacity
    // must go through bytes so both speak one unit.
    const expected = `${formatSize(Math.round((58.3 * 1e9) / 1024))} / ${formatSize(
      Math.round((214.75 * 1e9) / 1024),
    )}`;
    expect(screen.getByTitle(expected)).toBeTruthy();
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
