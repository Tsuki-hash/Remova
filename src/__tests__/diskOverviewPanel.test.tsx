// @vitest-environment jsdom
// Disk-space overview: volume shape, junction ledger with dead links,
// triage actions, regenerable cleanup, and on-demand DISM analysis.
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { DiskOverviewPanel } from "../components/DiskOverviewPanel";
import { api } from "../lib/api";
import { requestConfirmEx } from "../lib/confirm";
import { toast } from "../lib/toast";
import { t, formatSize } from "../i18n";

vi.mock("../lib/confirm", () => ({
  requestConfirmEx: vi.fn(async () => ({ ok: true, checked: false })),
}));
vi.mock("../lib/toast", () => ({
  toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() },
}));

vi.mock("../lib/api", () => ({
  api: {
    listLocalDrives: vi.fn(),
    junctionLedger: vi.fn(),
    spaceTriage: vi.fn(),
    regenerableLayouts: vi.fn(),
    deleteRegenerableLayouts: vi.fn(),
    analyzeComponentStore: vi.fn(),
    openPath: vi.fn(),
  },
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

function mockLoads() {
  vi.mocked(api.listLocalDrives).mockResolvedValue([
    { letter: "C:", free_gb: 50, total_gb: 200, is_system: true },
  ]);
  vi.mocked(api.junctionLedger).mockResolvedValue({
    entries: [
      { path: "c:\\Users\\a\\AppData\\Local\\Google", target: "d:\\moved\\Google", reachable: true },
      { path: "c:\\Program Files\\Docker", target: "d:\\docker", reachable: false },
    ],
    omitted: 3,
    degraded_roots: ["F:\\Gone"],
  });
  vi.mocked(api.spaceTriage).mockResolvedValue({
    entries: [
      {
        path: "C:\\Windows\\SoftwareDistribution\\Download",
        class: "delete_regenerable",
        reason: "win_update_download",
        size_kb: 812_500,
        size_capped: false,
      },
      {
        path: "C:\\hiberfil.sys",
        class: "show_only",
        reason: "system_swap_file",
        size_kb: 6_553_600,
        size_capped: false,
      },
      {
        path: "C:\\Windows\\Temp",
        class: "delete_regenerable",
        reason: "win_temp",
        size_kb: 1_200,
        size_capped: false,
      },
      {
        path: "C:\\Program Files\\Lenovo",
        class: "migrate_candidate",
        reason: "program_body",
        size_kb: 3_800_000,
        size_capped: false,
      },
    ],
    omitted: 0,
  });
  vi.mocked(api.regenerableLayouts).mockResolvedValue({
    items: [
      {
        path: "C:\\Windows\\Temp",
        reason: "win_temp",
        size_kb: 1_200,
        size_capped: false,
        deletable: true,
        block_reason: null,
      },
      {
        path: "C:\\Windows\\CbsTemp",
        reason: "cbs_temp",
        size_kb: 3_400,
        size_capped: false,
        deletable: false,
        block_reason: "servicing_busy",
      },
    ],
    omitted: 0,
  });
  vi.mocked(api.deleteRegenerableLayouts).mockResolvedValue({
    cleaned: ["C:\\Windows\\Temp"],
    skipped: [],
    reclaimed_kb: 1_200,
  });
}

it("renders the volume shape, dead junctions and degraded roots", async () => {
  mockLoads();
  render(<DiskOverviewPanel onClose={vi.fn()} />);
  const L = t();
  expect(await screen.findByText(L.diskOverviewJunctionSummary(2, 1))).toBeTruthy();
  expect(screen.getByText(L.diskOverviewSystemVolume("C:", "150.0 GB", "200.0 GB", "50.0 GB"))).toBeTruthy();
  expect(screen.getByRole("note").textContent).toContain("c:\\Program Files\\Docker");
  expect(screen.getByText(L.diskOverviewDegraded("F:\\Gone"))).toBeTruthy();
  expect(screen.getByText(L.scanOmitted(3))).toBeTruthy();
});

it("runs the DISM analysis on demand and reports the verdict", async () => {
  mockLoads();
  vi.mocked(api.analyzeComponentStore).mockResolvedValue({
    cleanup_recommended: true,
    explorer_size_bytes: 12_900_000_000,
    actual_size_bytes: 8_450_000_000,
  });
  render(<DiskOverviewPanel onClose={vi.fn()} />);
  const L = t();
  fireEvent.click(await screen.findByRole("tab", { name: L.diskOverviewTabComponents }));
  fireEvent.click(screen.getByRole("button", { name: L.diskOverviewAnalyze }));
  await waitFor(() => expect(screen.getByRole("status")).toBeTruthy());
  expect(api.analyzeComponentStore).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("status").textContent).toContain(L.diskOverviewRecommendedYes);
});

it("renders the triage rows with class labels and sizes", async () => {
  mockLoads();
  render(<DiskOverviewPanel onClose={vi.fn()} />);
  const L = t();
  fireEvent.click(await screen.findByRole("tab", { name: L.diskOverviewTabCleanup }));
  expect(await screen.findByText("C:\\Windows\\SoftwareDistribution\\Download")).toBeTruthy();
  expect(screen.getAllByText(L.triageClassDelete).length).toBeGreaterThan(0);
  expect(screen.getByText(L.triageClassShowOnly)).toBeTruthy();
  // Size ordering is decided server-side; the panel renders what it receives.
  expect(screen.getByText("C:\\hiberfil.sys")).toBeTruthy();
});

it("splits the panel into tabs and renders class names as muted status text", async () => {
  mockLoads();
  render(<DiskOverviewPanel onClose={vi.fn()} />);
  const L = t();
  expect(
    (await screen.findByRole("tab", { name: L.diskOverviewTabOverview })).getAttribute(
      "aria-selected",
    ),
  ).toBe("true");
  // The overview offers a single hand-off to the actionable list.
  fireEvent.click(await screen.findByRole("button", { name: L.diskOverviewOpenActions(2) }));
  expect(screen.getByRole("tab", { name: L.diskOverviewTabCleanup }).getAttribute("aria-selected")).toBe(
    "true",
  );
  // Status labels carry the muted tone, never the accent link color.
  const migrateLabels = screen.getAllByText(L.triageClassMigrate);
  expect(migrateLabels[0]!.style.color).toBe("var(--muted)");
  expect(screen.queryByRole("button", { name: L.diskOverviewAnalyze })).toBeNull();
  fireEvent.click(screen.getByRole("tab", { name: L.diskOverviewTabComponents }));
  expect(screen.getByRole("button", { name: L.diskOverviewAnalyze })).toBeTruthy();
});

it("keeps the scope note on the overview", async () => {
  mockLoads();
  render(<DiskOverviewPanel onClose={vi.fn()} />);
  expect(await screen.findByText(t().diskOverviewScopeNote)).toBeTruthy();
});

it("pins cleanable rows to the top of the action list", async () => {
  mockLoads();
  render(<DiskOverviewPanel onClose={vi.fn()} />);
  const L = t();
  fireEvent.click(await screen.findByRole("tab", { name: L.diskOverviewTabCleanup }));
  const items = await screen.findAllByRole("listitem");
  const text = items.map((i) => i.textContent ?? "");
  // The one deletable layout leads, even though the server sorted by size.
  expect(text[0]).toContain("C:\\Windows\\Temp");
  // Blocked deletes still precede migration candidates.
  const blocked = text.findIndex((x) => x.includes("C:\\Windows\\CbsTemp"));
  const migrate = text.findIndex((x) => x.includes("C:\\Program Files\\Lenovo"));
  expect(blocked).toBeGreaterThan(0);
  expect(blocked).toBeLessThan(migrate);
  // Leave/display-only rows sink to the bottom.
  expect(text[text.length - 1]).toContain("C:\\hiberfil.sys");
});

it("offers regenerable cleanup with a no-backup confirm and reports the result", async () => {
  mockLoads();
  render(<DiskOverviewPanel onClose={vi.fn()} />);
  const L = t();
  fireEvent.click(await screen.findByRole("tab", { name: L.diskOverviewTabCleanup }));
  // The blocked layout is visibly disabled with its reason. The row renders
  // the reason after a dash prefix, so match the key's text rather than the
  // whole line.
  expect(await screen.findByText(new RegExp(L.regenBlockServicing))).toBeTruthy();
  // Scope by the checkbox name: the triage row carries the same path.
  fireEvent.click(screen.getByRole("checkbox", { name: /C:\\Windows\\Temp/ }));
  fireEvent.click(screen.getByRole("button", { name: L.regenClean }));
  // The confirm restates the no-backup contract before anything runs.
  await waitFor(() => expect(requestConfirmEx).toHaveBeenCalledTimes(1));
  const opts = vi.mocked(requestConfirmEx).mock.calls[0]![0];
  expect(opts.message).toBe(L.regenCleanConfirm(1, formatSize(1200)));
  await waitFor(() =>
    expect(api.deleteRegenerableLayouts).toHaveBeenCalledWith(["C:\\Windows\\Temp"]),
  );
  await waitFor(() => expect(toast.success).toHaveBeenCalled());
  expect(vi.mocked(toast.success).mock.calls[0]![0]).toContain(L.regenDone(1, "1.17 MB").slice(0, 6));
});

it("acts from the triage rows: clean a regenerable layout and hand a migration candidate over", async () => {
  mockLoads();
  const onMigrate = vi.fn();
  render(<DiskOverviewPanel onClose={vi.fn()} onMigrate={onMigrate} />);
  const L = t();
  fireEvent.click(await screen.findByRole("tab", { name: L.diskOverviewTabCleanup }));
  // Cleaning from the row runs the same guarded confirm + delete path.
  fireEvent.click(
    await screen.findByRole("button", { name: `${L.triageActionClean}: C:\\Windows\\Temp` }),
  );
  await waitFor(() =>
    expect(api.deleteRegenerableLayouts).toHaveBeenCalledWith(["C:\\Windows\\Temp"]),
  );
  // Migration candidates are handed to the migration tool with the path.
  fireEvent.click(
    screen.getByRole("button", {
      name: `${L.triageActionMigrate}: C:\\Program Files\\Lenovo`,
    }),
  );
  expect(onMigrate).toHaveBeenCalledWith("C:\\Program Files\\Lenovo");
});

it("opens the parent folder of a dead junction and the folder of an action row", async () => {
  mockLoads();
  vi.mocked(api.openPath).mockResolvedValue(undefined);
  render(<DiskOverviewPanel onClose={vi.fn()} />);
  const L = t();
  fireEvent.click(
    await screen.findByRole("button", { name: `${L.openLocation}: c:\\Program Files\\Docker` }),
  );
  await waitFor(() => expect(api.openPath).toHaveBeenCalledWith("c:\\Program Files"));
  fireEvent.click(screen.getByRole("tab", { name: L.diskOverviewTabCleanup }));
  fireEvent.click(
    screen.getByRole("button", {
      name: `${L.openLocation}: C:\\Windows\\SoftwareDistribution\\Download`,
    }),
  );
  await waitFor(() =>
    expect(api.openPath).toHaveBeenCalledWith("C:\\Windows\\SoftwareDistribution\\Download"),
  );
});
