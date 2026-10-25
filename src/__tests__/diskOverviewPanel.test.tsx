// @vitest-environment jsdom
// Read-only disk-space overview: volume shape, junction ledger with dead
// links, and the on-demand DISM component-store analysis.
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { DiskOverviewPanel } from "../components/DiskOverviewPanel";
import { api } from "../lib/api";
import { t } from "../i18n";

vi.mock("../lib/api", () => ({
  api: {
    listLocalDrives: vi.fn(),
    junctionLedger: vi.fn(),
    spaceTriage: vi.fn(),
    analyzeComponentStore: vi.fn(),
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
    ],
    omitted: 0,
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
  fireEvent.click(await screen.findByRole("button", { name: L.diskOverviewAnalyze }));
  await waitFor(() => expect(screen.getByRole("status")).toBeTruthy());
  expect(api.analyzeComponentStore).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("status").textContent).toContain(L.diskOverviewRecommendedYes);
});

it("renders the triage rows with class labels and sizes", async () => {
  mockLoads();
  render(<DiskOverviewPanel onClose={vi.fn()} />);
  const L = t();
  expect(await screen.findByText("C:\\Windows\\SoftwareDistribution\\Download")).toBeTruthy();
  expect(screen.getByText(L.triageClassDelete)).toBeTruthy();
  expect(screen.getByText(L.triageClassShowOnly)).toBeTruthy();
  // Size ordering is decided server-side; the panel renders what it receives.
  expect(screen.getByText("C:\\hiberfil.sys")).toBeTruthy();
});

it("shows the read-only scope note so nothing here reads as actionable cleanup", async () => {
  mockLoads();
  render(<DiskOverviewPanel onClose={vi.fn()} />);
  expect(await screen.findByText(t().diskOverviewScopeNote)).toBeTruthy();
});
