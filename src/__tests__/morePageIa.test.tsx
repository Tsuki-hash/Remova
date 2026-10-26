// @vitest-environment jsdom
// More-page information architecture: five groups and a labelled search.
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MorePage } from "../components/MorePage";
import { t } from "../i18n";

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

it("groups the toolbox into the new sections and offers a search", async () => {
  mountMore();
  const L = t();
  // The search is labelled and explains itself with example keywords.
  expect(screen.getByText(L.moreSearchLabel)).toBeTruthy();
  expect(screen.getByLabelText(L.moreSearchTools)).toBeTruthy();
  expect(screen.getByPlaceholderText(L.moreSearchToolsHint)).toBeTruthy();
  expect(screen.getByText(L.moreSectionCommon)).toBeTruthy();
  expect(screen.getByText(L.moreSectionInstallHealth)).toBeTruthy();
  expect(screen.getByText(L.moreSectionSpace)).toBeTruthy();
  expect(screen.getByText(L.moreSectionTraces)).toBeTruthy();
  expect(screen.getByText(L.moreSectionHelp)).toBeTruthy();
  expect(screen.queryByText("进阶")).toBeNull();
  // The trace section carries the firewall card.
  expect(screen.getByText(L.firewallTitle)).toBeTruthy();
  // The old "recently used" section is gone for good.
  expect(screen.queryByText("最近使用")).toBeNull();
});

it("searching hides non-matching cards, reports the count and can be cleared", async () => {
  mountMore();
  const L = t();
  const search = screen.getByLabelText(L.moreSearchTools);
  fireEvent.change(search, { target: { value: L.firewallTitle } });
  expect(screen.getByText(L.firewallTitle)).toBeTruthy();
  expect(screen.queryByText(L.idleTitle)).toBeNull();
  expect(screen.queryByText(L.moreSectionSpace)).toBeNull();
  expect(screen.queryByText(L.moreSectionCommon)).toBeNull();
  expect(screen.getByRole("status").textContent).toBe(L.moreSearchResults(1));
  // Clearing brings every section back.
  fireEvent.click(screen.getByRole("button", { name: L.moreSearchClear }));
  expect(screen.getByText(L.moreSectionSpace)).toBeTruthy();
  expect(screen.getByText(L.moreSectionCommon)).toBeTruthy();
  expect(screen.queryByRole("status")).toBeNull();
});

it("shows an explicit empty state when nothing matches", async () => {
  mountMore();
  const L = t();
  fireEvent.change(screen.getByLabelText(L.moreSearchTools), {
    target: { value: "no-such-tool-xyz" },
  });
  expect(screen.getByText(L.moreSearchEmpty)).toBeTruthy();
  expect(screen.getByRole("status").textContent).toBe(L.moreSearchResults(0));
  // Escape clears the query as well.
  fireEvent.keyDown(screen.getByLabelText(L.moreSearchTools), { key: "Escape" });
  expect(screen.queryByText(L.moreSearchEmpty)).toBeNull();
  expect(screen.getByText(L.moreSectionCommon)).toBeTruthy();
});
