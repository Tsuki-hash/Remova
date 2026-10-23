// @vitest-environment jsdom
// Regression net for the PERF findings. These assert the *mechanism* of each fix
// (call counts, observer options, element overflow) rather than wall-clock time,
// which is exactly what made the defects invisible before.
import { readFileSync } from "node:fs";
import { act, cleanup, render, renderHook, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useAppFilter } from "../hooks/useAppFilter";
import { useAppChrome } from "../hooks/useAppChrome";
import { useListViewportHeight } from "../hooks/useListViewportHeight";
import { ManageListPage } from "../components/ManageListPage";
import type { InstalledApp, ManageItem } from "../types";

vi.mock("../lib/api", () => ({
  api: {
    listServices: vi.fn(async () => [] as ManageItem[]),
    listStartupItems: vi.fn(async () => [] as ManageItem[]),
    listScheduledTasks: vi.fn(async () => [] as ManageItem[]),
    isElevated: vi.fn(async () => true),
    elevateRestart: vi.fn(async () => {}),
    setServiceRunning: vi.fn(async () => {}),
    setServiceStartDisabled: vi.fn(async () => {}),
    setStartupEnabled: vi.fn(async () => {}),
    setTaskEnabled: vi.fn(async () => {}),
    beginAssociationScan: vi.fn(async () => 601),
    cancelAssociationScan: vi.fn(async () => true),
    associationScanProgress: vi.fn(async () => null),
  },
}));
vi.mock("../lib/confirm", () => ({ requestConfirm: vi.fn(async () => true) }));
vi.mock("../lib/toast", () => ({ toast: { info: vi.fn(), error: vi.fn(), success: vi.fn() } }));

// performance fix: count `recommendScore` evaluations without changing its behaviour.
const decision = vi.hoisted(() => ({ recommendScore: vi.fn() }));
vi.mock("../lib/decision", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../lib/decision")>();
  decision.recommendScore.mockImplementation(actual.recommendScore);
  return { ...actual, recommendScore: decision.recommendScore };
});

afterEach(() => { cleanup(); vi.clearAllMocks(); });

const app = (name: string, over: Partial<InstalledApp> = {}): InstalledApp => ({
  name, version: "1", publisher: "Vendor", install_location: `C:\\Program Files\\${name}`,
  uninstall_string: "x", quiet_uninstall_string: "", source: "HKLM64", registry_key: `HKLM\\${name}`,
  estimated_size_kb: 1024, install_date: "2024-01-15", display_icon: "", ...over,
});

/** A row with three-line task text — the shape performance fix clipped. */
const task = (over: Partial<ManageItem> = {}): ManageItem => ({
  name: "Acme Backup Task", detail: "C:\\Program Files\\Acme\\backup.exe /verbose /log C:\\very\\long\\path\\that\\wraps",
  location: "\\Acme\\Backup", enabled: true, kind: "task",
  last_run: "2026-01-01 10:00:00", next_run: "2026-02-01 10:00:00", ...over,
});

describe("sort scores each row once", () => {
  it("calls recommendScore O(n), not O(n log n)", () => {
    const apps = Array.from({ length: 60 }, (_, i) => app(`App ${String(i).padStart(2, "0")}`));
    const { result } = renderHook(() => useAppFilter({
      apps, copilotList: null, deferredQ: "", category: "all", sortCol: "recommend", sortDesc: false,
      sizeOf: (a) => a.estimated_size_kb, ignorePub: [], ignoreName: [],
      setSortCol: vi.fn(), setSortDesc: vi.fn(),
    }));
    expect(result.current.filtered).toHaveLength(60);
    // A comparator-based sort would evaluate this ~n·log₂n ≈ 350 times.
    expect(decision.recommendScore.mock.calls.length).toBe(60);
  });

  it("keeps the recommend column ordered by descending score", () => {
    const apps = [
      app("Huge", { estimated_size_kb: 4 * 1024 * 1024, install_date: "2020-01-01" }),
      app("Tiny", { estimated_size_kb: 1024, install_date: "2024-01-01" }),
      app("NoCmd", { estimated_size_kb: 4 * 1024 * 1024, uninstall_string: "", quiet_uninstall_string: "", install_date: "2020-01-01" }),
    ];
    const { result } = renderHook(() => useAppFilter({
      apps, copilotList: null, deferredQ: "", category: "all", sortCol: "recommend", sortDesc: false,
      sizeOf: (a) => a.estimated_size_kb, ignorePub: [], ignoreName: [],
      setSortCol: vi.fn(), setSortDesc: vi.fn(),
    }));
    expect(result.current.filtered.map(a => a.name)).toEqual(["Huge", "Tiny", "NoCmd"]);
  });

  it("keeps size and name ordering intact", () => {
    const apps = [
      app("Beta", { estimated_size_kb: 100 }),
      app("Alpha", { estimated_size_kb: 900 }),
    ];
    const base = { apps, copilotList: null, deferredQ: "", category: "all" as const,
      sizeOf: (a: InstalledApp) => a.estimated_size_kb, ignorePub: [], ignoreName: [],
      setSortCol: vi.fn(), setSortDesc: vi.fn() };
    const size = renderHook(() => useAppFilter({ ...base, sortCol: "size", sortDesc: false }));
    expect(size.result.current.filtered.map(a => a.name)).toEqual(["Beta", "Alpha"]);
    const name = renderHook(() => useAppFilter({ ...base, sortCol: "name", sortDesc: false }));
    expect(name.result.current.filtered.map(a => a.name)).toEqual(["Alpha", "Beta"]);
  });
});

describe("the appKey map is only built when the copilot branch runs", () => {
  it("resolves a copilot selection against the latest row objects", () => {
    const apps = [app("Alpha"), app("Beta")];
    const base = { apps, deferredQ: "", category: "all" as const, sortCol: null, sortDesc: false,
      sizeOf: (a: InstalledApp) => a.estimated_size_kb, ignorePub: [], ignoreName: [],
      setSortCol: vi.fn(), setSortDesc: vi.fn() };
    const plain = renderHook(() => useAppFilter({ ...base, copilotList: null }));
    const picked = renderHook(() => useAppFilter({ ...base, copilotList: [apps[1]!, apps[0]!] }));
    expect(plain.result.current.filtered.map(a => a.name)).toEqual(["Alpha", "Beta"]);
    // No sort column: the copilot branch preserves the order it was given and
    // resolves each entry to the newest row object for that identity.
    expect(picked.result.current.filtered.map(a => a.name)).toEqual(["Beta", "Alpha"]);
    expect(picked.result.current.filtered[0]).toBe(apps[1]);
  });

  it("drops copilot entries whose app is no longer in the list", () => {
    const apps = [app("Alpha"), app("Beta")];
    const { result } = renderHook(() => useAppFilter({
      apps, copilotList: [app("Gone")], deferredQ: "", category: "all", sortCol: null, sortDesc: false,
      sizeOf: (a) => a.estimated_size_kb, ignorePub: [], ignoreName: [],
      setSortCol: vi.fn(), setSortDesc: vi.fn(),
    }));
    expect(result.current.filtered).toEqual([]);
  });
});

describe("grouped setter bags keep stable identities", () => {
  const flow = {
    setIgnorePub: vi.fn(), setIgnoreName: vi.fn(), setError: vi.fn(),
    setScan: vi.fn(), setSelected: vi.fn(),
  };
  const residual = {
    clearSelection: vi.fn(), setMonitoring: vi.fn(), setMonitorDiff: vi.fn(),
    monitorDiff: null, selectDefaultItems: vi.fn(),
  };
  const selected = app("Acme");

  it("doIgnoreApp / toggleMonitor keep their identity across unrelated re-renders", () => {
    const { result, rerender } = renderHook(
      ({ sel }: { sel: InstalledApp }) => useAppChrome({
        L: undefined as never, selected: sel, monitoring: false, goNav: vi.fn(),
        flow, residual, setCheckupOrphanCount: vi.fn(),
      }),
      { initialProps: { sel: selected } },
    );
    const before = result.current.doIgnoreApp;
    const monitor = result.current.toggleMonitor;
    // `L` is the only value that is not memoized upstream; everything the hook
    // closes over here is stable, so the callbacks must not churn.
    rerender({ sel: selected });
    expect(result.current.doIgnoreApp).toBe(before);
    expect(result.current.toggleMonitor).toBe(monitor);
    // A real input change still produces a new callback.
    rerender({ sel: app("Other") });
    expect(result.current.doIgnoreApp).not.toBe(before);
  });

  it("App.tsx passes memoized bags instead of inline object literals", () => {
    // cwd-relative: `import.meta.url` is not a file URL under vi.mock factories.
    const src = readFileSync("src/App.tsx", "utf8");
    // The three flow hooks each received a fresh `flow: { … }` / `residual: { … }`
    // literal on every render, which is what made SoftwarePage's memo useless.
    expect(src).not.toMatch(/flow: \{/);
    expect(src).not.toMatch(/residual: \{/);
    for (const name of ["cleanupFlow", "analyzeFlow", "chromeFlow", "chromeResidual"]) {
      expect(src, `App.tsx: ${name} missing`).toContain(`const ${name} = useMemo(`);
    }
  });
});

describe("the viewport observer stops reacting to text and to the list itself", () => {
  const observed: { options: MutationObserverInit; cb: MutationCallback }[] = [];
  class SpyObserver {
    constructor(private readonly cb: MutationCallback) {}
    observe(target: Node, options: MutationObserverInit) { void target; observed.push({ options, cb: this.cb }); }
    disconnect() {}
    takeRecords() { return []; }
  }
  beforeEach(() => { observed.length = 0; vi.stubGlobal("MutationObserver", SpyObserver); });

  const host = () => {
    const main = document.createElement("main");
    const list = document.createElement("div");
    main.appendChild(list);
    document.body.appendChild(main);
    return { main, list };
  };

  it("observes childList only, never characterData", () => {
    const { main, list } = host();
    renderHook(() => useListViewportHeight({ current: list }));
    expect(observed).toHaveLength(1);
    expect(observed[0]!.options.childList).toBe(true);
    // Text mutations fired on every keystroke under <main>; the list's own row
    // swaps fired childList per scroll frame. Both are now filtered or dropped.
    expect(observed[0]!.options.characterData).toBeUndefined();
    expect(main.isConnected).toBe(true);
  });

  it("ignores mutations that come from inside the list itself", async () => {
    const { main, list } = host();
    // Count re-measures by counting rect reads on the list itself.
    const rect = vi.spyOn(list, "getBoundingClientRect");
    renderHook(() => useListViewportHeight({ current: list }));
    const before = rect.mock.calls.length;
    expect(before, "setup should measure once").toBeGreaterThan(0);
    const cb = observed[0]!.cb;
    const mutation = (target: Node) =>
      [{ type: "childList", target, addedNodes: [document.createElement("div")] } as unknown as MutationRecord];
    // A row the virtualizer swapped in: no re-measure.
    act(() => { cb(mutation(list), {} as MutationObserver); });
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 40)); });
    expect(rect.mock.calls.length).toBe(before);
    // A panel inserted above the list: re-measure.
    act(() => { cb(mutation(main), {} as MutationObserver); });
    await waitFor(() => expect(rect.mock.calls.length).toBeGreaterThan(before));
  });
});

describe("long manage rows are clipped instead of overlapping", () => {
  // The page drives @tanstack/react-virtual directly, and jsdom reports zero
  // layout, so give the virtualizer a measurable viewport.
  const realRect = Element.prototype.getBoundingClientRect;
  beforeEach(() => {
    Element.prototype.getBoundingClientRect = () =>
      ({ top: 0, left: 0, bottom: 600, right: 320, width: 320, height: 600, x: 0, y: 0, toJSON: () => ({}) });
    Object.defineProperty(HTMLElement.prototype, "offsetHeight", { configurable: true, get: () => 600 });
    Object.defineProperty(HTMLElement.prototype, "offsetWidth", { configurable: true, get: () => 320 });
  });
  afterEach(() => {
    Element.prototype.getBoundingClientRect = realRect;
    delete (HTMLElement.prototype as unknown as Record<string, unknown>).offsetHeight;
    delete (HTMLElement.prototype as unknown as Record<string, unknown>).offsetWidth;
  });

  it("clips the row and its text blocks so nothing spills into a neighbour", async () => {
    const { api } = await import("../lib/api");
    vi.mocked(api.listScheduledTasks).mockResolvedValue([task()]);
    const { container } = render(<ManageListPage tab="tasks" title="Tasks" onError={vi.fn()} />);
    const row = await waitFor(() => {
      const found = container.querySelector<HTMLElement>(".remova-manage-row");
      if (!found) throw new Error("row not rendered");
      return found;
    });
    expect(row.style.overflow).toBe("hidden");
    // The text column and the status column clip too, so a three-line task
    // description cannot push its siblings out of the 72px row.
    const columns = Array.from(row.children).filter(el => (el as HTMLElement).style.overflow === "hidden");
    expect(columns.length).toBeGreaterThanOrEqual(2);
    expect(screen.getByText(/Acme Backup Task/)).toBeTruthy();
  });
});
