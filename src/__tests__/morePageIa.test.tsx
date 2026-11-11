// @vitest-environment jsdom
// More-page information architecture: five groups and a labelled search.
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { MorePage } from "../components/MorePage";
import { t } from "../i18n";
import { requestBackupSession } from "../lib/backupSessionNavigation";

vi.mock("../lib/api", () => ({
  api: {
    openPath: vi.fn().mockResolvedValue(undefined),
    rebootGateStatus: vi.fn().mockResolvedValue("clear"),
    backupSessions: vi.fn().mockResolvedValue([]),
    history: vi.fn().mockResolvedValue([]),
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
  // The trace section carries the firewall card.
  expect(screen.getByText(L.firewallTitle)).toBeTruthy();
  // The toolbox renders exactly the five fixed sections (plus the close-mode
  // helper). This pin is structural, not a string match: sections that were
  // removed ("进阶", "最近使用") must stay gone whatever wording a revival
  // would use, so every rendered section title must come from the known set.
  const knownTitles: string[] = [
    L.moreSectionCommon,
    L.moreSectionInstallHealth,
    L.moreSectionSpace,
    L.moreSectionTraces,
    L.moreSectionHelp,
    L.closeMode,
  ];
  const sectionTitles = Array.from(
    document.querySelectorAll("section.remova-tool-section > header > span:first-child"),
  ).map((el) => el.textContent);
  expect(sectionTitles.filter((title) => !knownTitles.includes(title ?? ""))).toEqual([]);
  for (const fixed of knownTitles.slice(0, 5)) {
    expect(
      sectionTitles.filter((title) => title === fixed),
      `${fixed} must render exactly once`,
    ).toHaveLength(1);
  }
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

it.each([
  ["history", "panelClose"],
  ["restore", "restoreClose"],
  ["monitorInstall", "batchDismiss"],
] as const)("returns keyboard focus after closing the %s tool", async (title, closeLabel) => {
  mountMore();
  const L = t();
  const opener = screen.getByText(L[title]).closest("button")!;
  opener.focus();
  fireEvent.click(opener);
  const panel = screen.getByRole("region", { name: L.toolboxTitle });
  expect(document.activeElement).toBe(panel);
  const close = await within(panel).findByRole("button", { name: L[closeLabel] });
  close.focus();
  fireEvent.click(close);
  expect(document.activeElement).toBe(opener);
});

it("focuses tool search after closing restore opened from another page", async () => {
  mountMore();
  const L = t();
  await act(async () => { requestBackupSession("other-page-backup"); });
  const panel = screen.getByRole("region", { name: L.toolboxTitle });
  const close = within(panel).getByRole("button", { name: L.restoreClose });
  close.focus();
  fireEvent.click(close);
  expect(document.activeElement).toBe(screen.getByLabelText(L.moreSearchTools));
});
