// @vitest-environment jsdom
import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { AppDetailPanel } from "../components/AppDetailPanel";
import { formatSize, setLang, t } from "../i18n";
import { appKey } from "../lib/appKey";
import type { CleanupItem, InstalledApp, ScanResult } from "../types";

vi.mock("../lib/api", () => ({ api: { appIconData: vi.fn(async () => null) } }));

const app: InstalledApp = {
  name: "Editor", version: "2", publisher: "Example", install_location: "D:\\Apps\\Editor",
  uninstall_string: "uninstall.exe", quiet_uninstall_string: "", source: "HKLM64",
  registry_key: "Editor.2", estimated_size_kb: 100_000, install_date: "", display_icon: "",
};
const install: CleanupItem = {
  path: app.install_location, kind: "dir", confidence: "confirmed", risk: "low", score: 90,
  reason: "Install location", size_kb: 150_000, size_capped: true,
  evidence: [{ code: "install_location", label: "Install location", detail: app.install_location, weight: 90 }],
};
const project: CleanupItem = { ...install, path: "D:\\Projects\\Launch video", risk: "high", user_data: true,
  evidence: [{ code: "test_project", label: "Project manifest", detail: "launch.project", weight: 60 }] };
const shared: CleanupItem = { ...install, path: "D:\\Common\\Plugins", risk: "medium", shared: true, evidence: [] };
const suspect: CleanupItem = { ...install, path: "D:\\Old\\Editor", confidence: "suspected", evidence: [] };
const scan: ScanResult = { app_name: app.name, app_key: appKey(app), items: [install, suspect, project, shared] };

function panel(shownScan?: ScanResult) {
  return <AppDetailPanel app={app} sizeText="—" uninstalling={false} scan={shownScan}
    onAnalyze={vi.fn()} onDeepUninstall={vi.fn()} />;
}

afterEach(() => { cleanup(); setLang("zh"); });

it.each([
  ["zh", "优先核对的关联项", "查看测量范围"],
  ["en", "Associations to review first", "View measurement scope"],
] as const)("shows priority associations and the installation measurement scope (%s)", (language, review, scope) => {
  setLang(language);
  render(panel(scan));
  const L = t();
  const priority = screen.getByRole("region", { name: review });
  const items = priority.querySelectorAll("details");
  expect(items).toHaveLength(2);
  expect(items[0]?.querySelector("summary")?.textContent).toContain("Launch video");
  expect(items[0]?.querySelector("summary")?.textContent).toContain(L.reasonUserData);
  expect(items[1]?.querySelector("summary")?.textContent).toContain(L.reasonShared);
  expect(items[0]?.getAttribute("open")).toBeNull();
  expect(items[0]?.textContent).toContain(project.path);
  expect(items[0]?.textContent).toContain("launch.project");
  expect(screen.getByText(L.detailSizeFloor(formatSize(install.size_kb!)))).toBeTruthy();
  expect(screen.getByText(`${L.detailRegisteredSize} ${formatSize(app.estimated_size_kb)}`)).toBeTruthy();
  expect(screen.getByText(scope).closest("details")?.textContent).toContain(app.install_location);
});

it("only exposes review evidence for the matching installation", () => {
  const { rerender } = render(panel(scan));
  const region = screen.getByRole("region", { name: "优先核对的关联项" });
  expect(within(region).getByText("Launch video")).toBeTruthy();
  rerender(panel({ ...scan, app_key: appKey({ ...app, registry_key: "Other.Editor" }) }));
  expect(screen.queryByRole("region", { name: "优先核对的关联项" })).toBeNull();
  expect(screen.queryByText("查看测量范围")).toBeNull();
  expect(screen.getByText(t().linkedNotScanned)).toBeTruthy();
});
