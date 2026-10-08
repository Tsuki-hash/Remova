// @vitest-environment jsdom
// The toolbox keeps an always-visible Releases entry beside the update check.
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MorePage } from "../components/MorePage";
import { api } from "../lib/api";
import { RELEASES_URL } from "../lib/updateCheck";
import { t } from "../i18n";
import { requestBackupSession } from "../lib/backupSessionNavigation";

vi.mock("../lib/api", () => ({
  api: { openPath: vi.fn().mockResolvedValue(undefined),
    backupSessions: vi.fn().mockResolvedValue([{ name: "newest", size_kb: 1, created_at: "2" }, { name: "requested", size_kb: 1, created_at: "1" }]),
    previewRestore: vi.fn(async (name: string) => ({ name, files: 0, registry: 0, path_entries: 0, existing: 0, unavailable: 0, entries: [] })),
    restoreSessionByName: vi.fn() },
}));

afterEach(cleanup);

it("opens the Releases page from the help section header", async () => {
  render(
    <MorePage
      selected={null}
      monitoring={false}
      monitorDiff={null}
      lastReport={null}
      closeMode={null}
      onCloseModeChange={vi.fn()}
      onIgnorePublisher={vi.fn()}
      onToggleMonitor={vi.fn()}
      onMonitorToCleanup={vi.fn()}
      onDismissMonitor={vi.fn()}
      onExportReport={vi.fn()}
      onError={vi.fn()}
      onCheckUpdate={vi.fn()}
      onGoSoftware={vi.fn()}
    />,
  );
  fireEvent.click(await screen.findByRole("button", { name: t().openReleases }));
  await waitFor(() => expect(api.openPath).toHaveBeenCalledWith(RELEASES_URL));
  requestBackupSession("requested");
  await waitFor(() => expect(api.previewRestore).toHaveBeenCalledWith("requested"));
  expect(api.previewRestore).not.toHaveBeenCalledWith("newest");
  expect(api.restoreSessionByName).not.toHaveBeenCalled();
});
