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
  // mockReset (not clearAllMocks): mockResolvedValueOnce queues and mock
  // implementations survive clearAllMocks and would leak across tests.
  vi.mocked(api.migrateStatus).mockReset();
  vi.clearAllMocks();
});

function type(src = "C:\\src", dst = "D:\\dst") {
  fireEvent.change(screen.getByLabelText(t().migrateSource), { target: { value: src } });
  fireEvent.change(screen.getByLabelText(t().migrateTarget), { target: { value: dst } });
}

it("renders plan rejections in the user's language", async () => {
  // Mount consumes the first (idle); the post-copy refreshState consumes
  // the standing "copied" answer, mirroring the real backend.
  vi.mocked(api.migrateStatus).mockResolvedValueOnce("idle").mockResolvedValue("copied");
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
  fireEvent.click(await screen.findByRole("button", { name: t().migrateStart }));
  await waitFor(() => expect(api.migratePlan).toHaveBeenCalled());
  const L = t();
  await waitFor(() =>
    expect(screen.getByText(L.migrateRejSquirrel)).toBeTruthy(),
  );
  // Unknown codes fall back to the raw code instead of disappearing.
  expect(screen.getByText("made_up_code")).toBeTruthy();
});

it("gates destructive phases on the derived state and confirms cutover", async () => {
  // Mount sees "copied"; the post-cutover refreshState sees "cutover".
  vi.mocked(api.migrateStatus)
    .mockResolvedValueOnce("copied")
    .mockResolvedValue("cutover");
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
  // The status probe is debounced (300 ms) — poll for the stepper's current
  // step (copy done -> cutover is next) instead of a sync query.
  await waitFor(() => {
    const step = screen.getByRole("list", { name: L.migrateTitle }).querySelector('[aria-current="step"]');
    if (!step || !step.textContent?.includes(L.stepCutover)) {
      throw new Error("cutover step must become current after the copy");
    }
  }, { timeout: 3000 });
  const cutover = screen.getByRole("button", { name: L.migrateCutover });
  expect(cutover).toHaveProperty("disabled", false);
  // In the copied state the finish is not offered at all (single CTA).
  expect(screen.queryByRole("button", { name: L.migrateFinish })).toBeNull();
  fireEvent.click(cutover);
  await waitFor(() => expect(requestConfirmEx).toHaveBeenCalled());
  await waitFor(() => expect(api.migrateCutover).toHaveBeenCalled());
  await waitFor(() => expect(toast.success).toHaveBeenCalled());
  // After the cutover the single CTA becomes the finish; rollback appears.
  await waitFor(() => {
    const finish = screen.getByRole("button", { name: L.migrateFinish });
    if (finish.hasAttribute("disabled")) throw new Error("finish must unlock after the cutover");
  });
  expect(screen.getByRole("button", { name: L.migrateRollback })).toBeTruthy();
});

it("the primary path starts from idle: plan ok enables the copy button", async () => {
  // Regression: the copy gate once required state === "copied", which locked
  // a first-run user out of the very first copy.
  // Mount consumes the first (idle); the post-copy refreshState consumes
  // the standing "copied" answer, mirroring the real backend.
  vi.mocked(api.migrateStatus).mockResolvedValueOnce("idle").mockResolvedValue("copied");
  vi.mocked(api.migratePlan).mockResolvedValue({
    source: "C:\\src", target: "D:\\dst", ok: true,
    rejections: [], source_kb: 10, file_count: 2, target_free_kb: 10_000_000,
  });
  vi.mocked(api.migrateCopy).mockResolvedValue({
    files: 2, source_kb: 10, files_capped: false, reparse_skipped: [],
  });
  render(<MigratePanel onClose={vi.fn()} />);
  const L = t();
  type();
  fireEvent.click(await screen.findByRole("button", { name: L.migrateStart }));
  // The wizard flows straight into the copy; wait for it to complete.
  await waitFor(() => expect(api.migrateCopy).toHaveBeenCalled());
  await waitFor(() => expect(api.migrateCopy).toHaveBeenCalledWith("C:\\src", "D:\\dst"));
  await waitFor(() => expect(toast.success).toHaveBeenCalled());
  // After the copy lands the derived state is "copied": cutover unlocks.
  await waitFor(() => {
    const cutover = screen.getByRole("button", { name: L.migrateCutover });
    if (cutover.hasAttribute("disabled")) throw new Error("cutover must unlock");
  });
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
    const cutover = screen.getByRole("button", { name: t().migrateCutover });
    if (cutover.hasAttribute("disabled")) throw new Error("not ready");
  });
  fireEvent.click(screen.getByRole("button", { name: t().migrateCutover }));
  await waitFor(() => expect(toast.error).toHaveBeenCalled());
  expect(vi.mocked(toast.error).mock.calls[0]![0]).toBe(t().migrateRolledBack);
});
