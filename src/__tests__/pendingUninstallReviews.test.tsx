// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { PendingUninstallReviews } from "../components/PendingUninstallReviews";
import { t } from "../i18n";
import { api } from "../lib/api";

vi.mock("../lib/api", () => ({ api: {
  pendingUninstallReviews: vi.fn(),
  checkPendingUninstallReview: vi.fn(),
  dismissPendingUninstallReview: vi.fn(),
} }));

afterEach(() => { cleanup(); vi.resetAllMocks(); });

it("rechecks a saved reminder and removes it only when the user dismisses it", async () => {
  const L = t();
  const orphans = vi.fn();
  const row = { id: 15, name: "Sample Editor", version: "1.0" };
  vi.mocked(api.pendingUninstallReviews).mockResolvedValue([row]);
  vi.mocked(api.checkPendingUninstallReview)
    .mockResolvedValueOnce("awaiting_restart")
    .mockResolvedValueOnce("unverified")
    .mockResolvedValueOnce("installed")
    .mockResolvedValueOnce("removed");
  vi.mocked(api.dismissPendingUninstallReview).mockImplementation(async () => {
    vi.mocked(api.pendingUninstallReviews).mockResolvedValue([]);
  });
  render(<PendingUninstallReviews busy={false} onGoOrphans={orphans} />);
  await screen.findByText("Sample Editor · 1.0");
  for (const label of [L.pendingReviewAwaitRestart, L.pendingReviewUnverified, L.pendingReviewInstalled, L.pendingReviewRemoved]) {
    fireEvent.click(screen.getByRole("button", { name: L.pendingReviewCheck }));
    await screen.findByText(label);
  }
  expect(api.checkPendingUninstallReview).toHaveBeenCalledTimes(4);
  fireEvent.click(screen.getByRole("button", { name: L.navOrphans }));
  expect(orphans).toHaveBeenCalledOnce();
  expect(api.dismissPendingUninstallReview).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: L.pendingReviewDismiss }));
  await waitFor(() => expect(screen.queryByText("Sample Editor · 1.0")).toBeNull());
  expect(api.dismissPendingUninstallReview).toHaveBeenCalledWith(row.id);
});
