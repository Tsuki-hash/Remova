// @vitest-environment jsdom
// More-page information architecture: five groups and a labelled search.
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MorePage } from "../components/MorePage";
import { ConfirmHost } from "../components/ui/ConfirmHost";
import { useMoreRestore } from "../hooks/useMoreRestore";
import { t } from "../i18n";
import { requestBackupSession } from "../lib/backupSessionNavigation";
import { api } from "../lib/api";
import { getConfirm, settleConfirm } from "../lib/confirm";

vi.mock("../lib/api", () => ({
  api: {
    openPath: vi.fn().mockResolvedValue(undefined),
    rebootGateStatus: vi.fn().mockResolvedValue("clear"),
    backupSessions: vi.fn().mockResolvedValue([]),
    history: vi.fn().mockResolvedValue([]),
    previewRestore: vi.fn(),
    cancelRestorePreview: vi.fn(async () => true),
    deleteBackupSession: vi.fn(),
    restoreSessionByName: vi.fn(),
    cancelRestore: vi.fn(),
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
  vi.mocked(api.backupSessions).mockReset().mockResolvedValue([]);
  vi.mocked(api.previewRestore).mockReset();
  vi.mocked(api.deleteBackupSession).mockReset();
  vi.mocked(api.restoreSessionByName).mockReset();
  vi.mocked(api.cancelRestore).mockReset();
});
afterEach(() => { settleConfirm(false); cleanup(); vi.clearAllMocks(); });

function Toolbox({ visible = true }: { visible?: boolean }) {
  const restore = useMoreRestore(props.onError);
  return <><ConfirmHost />{visible && <MorePage {...props} restore={restore} />}</>;
}

function mountMore() {
  return render(<Toolbox />);
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

const vaultSession = (name: string) => ({ name, size_kb: 2, created_at: "", sealed: true, status: "verified" as const });
const vaultPreview = (name: string) => ({ name, files: 1, registry: 0, path_entries: 0, existing: 0, unavailable: 0, entries: [] });

it.each(["Escape", "Cancel"] as const)("restores focus after dismissing restore confirmation with %s", async (method) => {
  const L = t();
  const name = "backup-for-restore";
  vi.mocked(api.backupSessions).mockResolvedValue([vaultSession(name)]);
  vi.mocked(api.previewRestore).mockResolvedValue(vaultPreview(name));
  mountMore();
  fireEvent.click(screen.getByText(L.restore).closest("button")!);
  const opener = await screen.findByRole("button", { name: L.restoreRun });
  await waitFor(() => expect(opener).toHaveProperty("disabled", false));
  opener.focus();
  fireEvent.click(opener);
  const dialog = await screen.findByRole("dialog", { name: L.restore });
  if (method === "Escape") fireEvent.keyDown(dialog, { key: "Escape" });
  else fireEvent.click(within(dialog).getByRole("button", { name: L.cancel }));
  await waitFor(() => expect(document.activeElement).toBe(opener));
  expect(api.restoreSessionByName).not.toHaveBeenCalled();
});

it.each(["neighbor", "empty", "failed", "moved"] as const)("returns keyboard focus after deleting a backup (%s)", async (outcome) => {
  const L = t();
  const name = "backup-to-delete";
  const next = "backup-next";
  const sessions = [vaultSession(name), ...(outcome === "empty" ? [] : [vaultSession(next)])];
  vi.mocked(api.backupSessions).mockResolvedValueOnce(sessions).mockResolvedValue(sessions.slice(1));
  vi.mocked(api.previewRestore).mockImplementation(async (session) => vaultPreview(session));
  let finish!: () => void;
  vi.mocked(api.deleteBackupSession).mockImplementation(() => new Promise<void>((resolve, reject) => {
    finish = () => outcome === "failed" ? reject(new Error("Denied")) : resolve();
  }));
  mountMore();
  fireEvent.click(screen.getByText(L.restore).closest("button")!);
  const opener = await screen.findByRole("button", { name: L.deleteSession + ": " + name });
  opener.focus();
  fireEvent.click(opener);
  await screen.findByRole("dialog", { name: L.deleteSession });
  await act(async () => settleConfirm(true));
  await waitFor(() => expect(api.deleteBackupSession).toHaveBeenCalledWith(name, "verified"));
  const search = screen.getByLabelText(L.moreSearchTools);
  if (outcome === "moved") search.focus();
  await act(async () => finish());
  const focusTarget = outcome === "failed" ? opener
    : outcome === "moved" ? search
    : outcome === "empty" ? screen.getByText(L.safetyVaultTitle)
    : screen.getByRole("radio", { name: new RegExp(next) });
  expect(document.activeElement).toBe(focusTarget);
});

it.each(["partial", "failure", "cancel", "cancel before writes"] as const)("keeps a %s restore result across navigation", async outcome => {
  const name = "backup-for-restore";
  const L = t();
  vi.mocked(api.backupSessions).mockResolvedValueOnce([{ name, size_kb: 2, created_at: "", sealed: true, status: "verified" }]);
  vi.mocked(api.previewRestore).mockResolvedValue({ name, files: 2, registry: 0, path_entries: 0, existing: 0, unavailable: 0, entries: [] });
  let finish!: () => void;
  vi.mocked(api.restoreSessionByName).mockImplementationOnce(() => new Promise((resolve, reject) => {
    finish = () => {
      if (outcome === "failure") reject(new Error("seal:map_mismatch"));
      else if (outcome === "cancel before writes") reject(new Error("restore cancelled before any changes"));
      else resolve(["restored first-file", outcome === "cancel" ? "restore cancelled" : "restore failed for second-file: access denied"]);
    };
  }));
  const cancel = outcome === "cancel" || outcome === "cancel before writes";
  if (cancel) vi.mocked(api.cancelRestore).mockResolvedValueOnce(true);
  const view = mountMore();
  fireEvent.click(screen.getByText(L.restore).closest("button")!);
  await waitFor(() => expect(screen.getByRole("button", { name: L.restoreRun })).toHaveProperty("disabled", false), { timeout: 3000 });
  fireEvent.click(screen.getByRole("button", { name: L.restoreRun }));
  await waitFor(() => expect(getConfirm()?.title).toBe(L.restore));
  await act(async () => settleConfirm(true));
  await waitFor(() => expect(api.restoreSessionByName).toHaveBeenCalledOnce());
  expect(screen.getByText(L.restoreRunning(name))).toBeTruthy();
  view.rerender(<Toolbox visible={false} />);
  view.rerender(<Toolbox />);
  expect(screen.getByText(L.restoreRunning(name))).toBeTruthy();
  if (cancel) {
    fireEvent.click(screen.getByRole("button", { name: L.restoreCancel }));
    const [calledName, operationId] = vi.mocked(api.restoreSessionByName).mock.calls[0]!;
    expect(calledName).toBe(name);
    expect(operationId).toMatch(/^[\da-f-]{36}$/);
    expect(api.cancelRestore).toHaveBeenCalledWith(operationId);
    expect(screen.getByText(L.restoreCancelling)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: L.restoreCancel }));
    expect(api.cancelRestore).toHaveBeenCalledOnce();
  }
  view.rerender(<Toolbox visible={false} />);
  await act(async () => finish());
  view.rerender(<Toolbox />);
  expect(screen.getByText(`${L.restoreResult}: ${name}`)).toBeTruthy();
  expect(screen.queryByRole("button", { name: L.restoreCancel })).toBeNull();
  if (outcome === "partial") expect(screen.getByText(/second-file: access denied/)).toBeTruthy();
  if (outcome === "failure") expect(screen.getByText(L.errSealInvalid)).toBeTruthy();
  if (outcome === "cancel") expect(screen.getByText(new RegExp(L.restoreFilesCancelled))).toBeTruthy();
  if (outcome === "cancel before writes") expect(screen.getByText(L.restoreCancelledBeforeChanges)).toBeTruthy();
});
