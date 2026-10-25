// @vitest-environment jsdom
// More-page information architecture: five groups, a filter when the toolbox
// grows past eight cards, and a persisted "recently used" section.
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MorePage } from "../components/MorePage";
import { t } from "../i18n";
import { loadRecentTools, saveRecentTools } from "../lib/recentTools";

vi.mock("../lib/api", () => ({
  api: {
    openPath: vi.fn().mockResolvedValue(undefined),
    rebootGateStatus: vi.fn().mockResolvedValue("clear"),
    backupSessions: vi.fn().mockResolvedValue([]),
    previewRestore: vi.fn(),
    restoreSessionByName: vi.fn(),
  },
}));

const props = {
  selected: null,
  monitoring: false,
  monitorDiff: null,
  lastReport: null,
  closeMode: null,
  onCloseModeChange: vi.fn(),
  onIgnorePublisher: vi.fn(),
  onToggleMonitor: vi.fn(),
  onMonitorToCleanup: vi.fn(),
  onDismissMonitor: vi.fn(),
  onExportReport: vi.fn(),
  onError: vi.fn(),
  onCheckUpdate: vi.fn(),
  onGoSoftware: vi.fn(),
};

beforeEach(() => {
  localStorage.clear();
});
afterEach(cleanup);

function mountMore() {
  return render(<MorePage {...props} />);
}

it("groups the toolbox into the new sections and offers a filter", async () => {
  mountMore();
  const L = t();
  expect(screen.getByPlaceholderText(L.moreFilterTools)).toBeTruthy();
  expect(screen.getByText(L.moreSectionCommon)).toBeTruthy();
  expect(screen.getByText(L.moreSectionInstallHealth)).toBeTruthy();
  expect(screen.getByText(L.moreSectionSpace)).toBeTruthy();
  expect(screen.getByText(L.moreSectionTraces)).toBeTruthy();
  expect(screen.getByText(L.moreSectionHelp)).toBeTruthy();
  expect(screen.queryByText("进阶")).toBeNull();
  // The trace section carries the firewall card.
  expect(screen.getByText(L.firewallTitle)).toBeTruthy();
  expect(screen.queryByText(L.moreSectionRecent)).toBeNull();
});

it("filtering hides non-matching cards and the sections left empty", async () => {
  mountMore();
  const L = t();
  fireEvent.change(screen.getByPlaceholderText(L.moreFilterTools), {
    target: { value: L.firewallTitle },
  });
  expect(screen.getByText(L.firewallTitle)).toBeTruthy();
  expect(screen.queryByText(L.idleTitle)).toBeNull();
  expect(screen.queryByText(L.moreSectionSpace)).toBeNull();
  expect(screen.queryByText(L.moreSectionCommon)).toBeNull();
});

it("records prop-only tools in the recently-used section, newest first", async () => {
  mountMore();
  const L = t();
  // Both of these cards run prop-only actions, so no panel api is touched.
  fireEvent.click(screen.getByText(L.onboardingReopen));
  fireEvent.click(screen.getByText(L.versionCheck));
  expect(screen.getByText(L.moreSectionRecent)).toBeTruthy();
  const recent = screen.getAllByText(L.onboardingReopen);
  expect(recent.length).toBe(2); // pinned copy plus the help-section card
  expect(loadRecentTools()).toEqual(["releases", "onboarding"]);
  // The persisted order survives a remount.
  cleanup();
  mountMore();
  expect(screen.getByText(L.moreSectionRecent)).toBeTruthy();
});

it("recent storage caps at three entries", () => {
  saveRecentTools(["a", "b", "c", "d"]);
  expect(loadRecentTools()).toEqual(["a", "b", "c"]);
  saveRecentTools([]);
  expect(loadRecentTools()).toEqual([]);
});
