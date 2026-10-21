// @vitest-environment jsdom
// Regression: the idle radar used to print the install date on the summary
// line while its "安装日期" label sat alone on the next line. The date now
// travels with its label and stays on one line.
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { IdleRadarPanel } from "../components/IdleRadarPanel";
import { setLang, t } from "../i18n";
import type { IdleApp } from "../types";

const row: IdleApp = {
  app: {
    name: "NVIDIA Nsight Systems 2023.1.2",
    version: "2023.1.2",
    publisher: "NVIDIA",
    install_location: "C:\\Program Files\\NVIDIA Corporation\\Nsight Systems",
    uninstall_string: "",
    quiet_uninstall_string: "",
    source: "HKLM64",
    registry_key: "nvidia-nsight",
    estimated_size_kb: 846_000,
    install_date: "2023-10-24",
    display_icon: "",
  },
  idle_days: 1093,
  size_kb: 846_000,
  evidence: [
    { code: "idle_install_age", detail: "install_date=2023-10-24" },
    { code: "idle_dir_mtime", detail: "1093d" },
  ],
  score: 42,
};

vi.mock("../lib/api", () => ({
  api: {
    beginAssociationScan: vi.fn(async () => 501),
    cancelAssociationScan: vi.fn(async () => {}),
    associationScanProgress: vi.fn(async () => null),
    rankIdleApps: vi.fn(async () => ({ items: [row], omitted: 0 })),
  },
}));
afterEach(cleanup);
beforeEach(() => { vi.clearAllMocks(); setLang("zh"); });

it("keeps the install date on the same line as its label", async () => {
  const L = t();
  render(<IdleRadarPanel onClose={vi.fn()} onGoSoftware={vi.fn()} onError={vi.fn()} />);
  await screen.findByText(row.app.name);
  // The label and the date are one nowrap element, so no line break can
  // split them and the summary line no longer carries a dangling date.
  expect(screen.getByText(`${L.idleEvInstallAge} ${row.app.install_date}`)).toBeTruthy();
  expect(screen.queryByText(L.idleEvInstallAge)).toBeNull();
  const rowText = screen.getByText(row.app.name).parentElement?.textContent ?? "";
  expect(rowText).toContain(L.idleDays(row.idle_days));
  expect(rowText).not.toContain(`· ${row.app.install_date}`);
});
