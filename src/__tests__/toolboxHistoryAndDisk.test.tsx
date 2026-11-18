// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { api } from "../lib/api";
import { MorePage } from "../components/MorePage";
import { useMoreRestore } from "../hooks/useMoreRestore";
import { setLang, t } from "../i18n";

vi.mock("../lib/api", () => ({ api: {
  rebootGateStatus: vi.fn(async () => "clear"),
  exportHistoryCsv: vi.fn(async () => "app_name,deleted\nUX-Probe,1\n# skipped_unreadable_rows,1\n"),
  history: vi.fn(async () => ({
    entries: [{ id: "demo", app_name: "UX-Probe", created_at: "1700000000", deleted: 1, failed: 0 }],
    corrupt_rows: 1,
  })),
  listLocalDrives: vi.fn(async () => [{ letter: "D", free_gb: 40, total_gb: 50, is_system: true }]),
  junctionLedger: vi.fn(async () => ({ entries: [], omitted: 0, degraded_roots: [] })),
  spaceTriage: vi.fn(async () => ({ entries: [], omitted: 0 })),
  regenerableLayouts: vi.fn(async () => ({ items: [], omitted: 0 })),
}}));

const noop = () => {};
function Toolbox() {
  const restore = useMoreRestore(noop);
  return <MorePage restore={restore} selected={null} monitoring={false} monitorDiff={null}
    lastReport={null} closeMode={null} onCloseModeChange={noop} onIgnorePublisher={async () => undefined}
    onToggleMonitor={noop} onMonitorToCleanup={noop} onDismissMonitor={noop} onExportReport={noop}
    onError={noop} onCheckUpdate={noop} onGoSoftware={noop} />;
}
afterEach(() => { cleanup(); setLang("zh"); });

it.each(["zh", "en"] as const)("exports populated history from the toolbox in %s", async lang => {
  setLang(lang);
  render(<Toolbox />);
  const L = t();
  fireEvent.click(screen.getByRole("button", { name: L.history }));
  expect(await screen.findByText("UX-Probe")).toBeTruthy();
  expect(screen.getByRole("alert").textContent).toContain("CSV");
  const objectUrl = vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:history");
  const revoke = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
  const download = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (this: HTMLAnchorElement) {
    expect(this.download).toBe("remova-history.csv");
  });
  try {
    fireEvent.click(screen.getByRole("button", { name: L.exportHistoryAllCsv }));
    await vi.waitFor(() => expect(download).toHaveBeenCalledOnce());
    expect(api.exportHistoryCsv).toHaveBeenCalled();
    expect(objectUrl).toHaveBeenCalledWith(expect.any(Blob));
    expect(revoke).toHaveBeenCalledWith("blob:history");
  } finally {
    objectUrl.mockRestore();
    revoke.mockRestore();
    download.mockRestore();
  }
});

it.each(["zh", "en"] as const)("system-volume heading adapts to a D system drive in %s", async lang => {
  setLang(lang);
  render(<Toolbox />);
  const L = t();
  fireEvent.click(screen.getByRole("button", { name: L.diskOverviewTitle }));
  expect(await screen.findByText(L.diskOverviewSystemVolume("D", "10.0 GB", "50.0 GB", "40.0 GB"))).toBeTruthy();
  expect(screen.getByRole("heading", { name: L.diskOverviewTitle }).textContent).not.toMatch(/^C/);
});
