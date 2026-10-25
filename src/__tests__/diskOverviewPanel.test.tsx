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

it("shows the read-only scope note so nothing here reads as actionable cleanup", async () => {
  mockLoads();
  render(<DiskOverviewPanel onClose={vi.fn()} />);
  expect(await screen.findByText(t().diskOverviewScopeNote)).toBeTruthy();
});
