// @vitest-environment jsdom
// Migration panel: plan refusals render, phases gate on the derived state,
// and destructive phases confirm first.
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MigratePanel } from "../components/MigratePanel";
import { api } from "../lib/api";
import { requestConfirmEx } from "../lib/confirm";
import { toast } from "../lib/toast";
import { t } from "../i18n";

vi.mock("../lib/api", () => ({
  api: {
    migratePlan: vi.fn(),
    migrateCopy: vi.fn(),
    migrateCutover: vi.fn(),
    migrateFinish: vi.fn(),
    migrateRollback: vi.fn(),
    migrateStatus: vi.fn(async () => "idle"),
    cancelMigrateCopy: vi.fn(),
  },
}));
vi.mock("../lib/confirm", () => ({
  requestConfirmEx: vi.fn(async () => ({ ok: true, checked: false })),
}));
vi.mock("../lib/toast", () => ({
  toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() },
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

function type(src = "C:\\src", dst = "D:\\dst") {
  fireEvent.change(screen.getByLabelText(t().migrateSource), { target: { value: src } });
  fireEvent.change(screen.getByLabelText(t().migrateTarget), { target: { value: dst } });
}

it("renders plan rejections in the user's language", async () => {
  vi.mocked(api.migrateStatus).mockResolvedValue("idle");
  vi.mocked(api.migratePlan).mockResolvedValue({
    source: "C:\\src",
    target: "D:\\dst",
    ok: false,
    rejections: ["source_squirrel_updater", "made_up_code"],
    source_kb: 0,
    file_count: 0,
    target_free_kb: 0,
  });
  render(<MigratePanel onClose={vi.fn()} />);
  type();
  fireEvent.click(await screen.findByRole("button", { name: t().migratePlanBtn }));
  await waitFor(() => expect(api.migratePlan).toHaveBeenCalled());
  const L = t();
  await waitFor(() =>
    expect(screen.getByText(L.migrateRejSquirrel)).toBeTruthy(),
  );
  // Unknown codes fall back to the raw code instead of disappearing.
  expect(screen.getByText("made_up_code")).toBeTruthy();
});

it("gates destructive phases on the derived state and confirms cutover", async () => {
  vi.mocked(api.migrateStatus).mockResolvedValue("copied");
  vi.mocked(api.migrateCutover).mockResolvedValue({
    ok: true,
    junction: "C:\\src",
    backup: "C:\\src.bak",
    rolled_back: false,
    error: null,
  });
  render(<MigratePanel onClose={vi.fn()} />);
  const L = t();
  type();
  await waitFor(() => expect(screen.getByText(L.migrateCurrentState(L.migrateStateCopied))).toBeTruthy());
  const cutover = screen.getByRole("button", { name: L.migrateCutover }) as HTMLButtonElement;
  expect(cutover.disabled).toBe(false);
  const finish = screen.getByRole("button", { name: L.migrateFinish }) as HTMLButtonElement;
  expect(finish.disabled).toBe(true);
  fireEvent.click(cutover);
  await waitFor(() => expect(requestConfirmEx).toHaveBeenCalled());
  await waitFor(() => expect(api.migrateCutover).toHaveBeenCalled());
  await waitFor(() => expect(toast.success).toHaveBeenCalled());
});

it("reports an automatic rollback as an error with the server reason", async () => {
  vi.mocked(api.migrateStatus).mockResolvedValue("copied");
  vi.mocked(api.migrateCutover).mockResolvedValue({
    ok: false,
    junction: "C:\\src",
    backup: "C:\\src.bak",
    rolled_back: true,
    error: "junction creation failed: boom",
  });
  render(<MigratePanel onClose={vi.fn()} />);
  type();
  await waitFor(() => {
    const cutover = screen.getByRole("button", { name: t().migrateCutover }) as HTMLButtonElement;
    if (cutover.disabled) throw new Error("not ready");
  });
  fireEvent.click(screen.getByRole("button", { name: t().migrateCutover }));
  await waitFor(() => expect(toast.error).toHaveBeenCalled());
  expect(vi.mocked(toast.error).mock.calls[0]![0]).toBe(t().migrateRolledBack);
});
