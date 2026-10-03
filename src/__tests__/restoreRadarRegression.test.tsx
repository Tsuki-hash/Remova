// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import { useMoreRestore } from "../hooks/useMoreRestore";
import { RestorePanel } from "../components/RestorePanel";
import { DiskRadarPanel } from "../components/DiskRadarPanel";
import { t } from "../i18n";
import { requestConfirm } from "../lib/confirm";
import { toast } from "../lib/toast";

const native = vi.hoisted(() => ({ previewRestore: vi.fn(), backupSessions: vi.fn(), deleteBackupSession: vi.fn(), restoreSessionByName: vi.fn(), listLocalDrives: vi.fn(), listTopDirSizes: vi.fn(), listDirChildren: vi.fn(), openPath: vi.fn() }));
vi.mock("../lib/api", () => ({ api: native }));
vi.mock("../lib/confirm", () => ({ requestConfirm: vi.fn(async () => true) }));
vi.mock("../lib/toast", () => ({ toast: { success: vi.fn(), info: vi.fn(), error: vi.fn() } }));
afterEach(cleanup);
beforeEach(() => {
  vi.clearAllMocks();
  native.backupSessions.mockResolvedValue([{ name: "older-backup", size_kb: 1, created_at: "" }]);
  native.previewRestore.mockImplementation(async (name: string) => ({ name, files: 1, registry: 0, path_entries: 0, existing: 0, unavailable: 0, entries: [] }));
  native.listLocalDrives.mockResolvedValue([{ letter: "C", is_system: true, total_gb: 1, free_gb: 1 }]);
  native.listTopDirSizes.mockResolvedValue([{ path: "root", name: "root", size_kb: 1, capped: true }]);
  native.openPath.mockResolvedValue(undefined);
});

it("keeps restore failures beyond the twentieth row in a bounded scroll area", () => {
  const messages = Array.from({ length: 25 }, (_, i) => `restore-row-${i + 1}`);
  const { container } = render(<RestorePanel sessions={[]} pick="" setPick={() => {}}
    busy={false} msgs={messages} onRun={() => {}} onDelete={() => {}} onClose={() => {}} />);
  const details = container.querySelector("pre")!;
  expect(details.textContent).toContain("restore-row-25");
  expect(details.style.maxHeight).toBe("240px");
  expect(details.style.overflow).toBe("auto");
});

it("names the selected backup and reports skipped or failed rows without a success toast", async () => {
  native.restoreSessionByName.mockResolvedValue(["restored file", "skipped protected restore target: protected", "restore failed for broken: denied"]);
  const { result } = renderHook(() => useMoreRestore(vi.fn()));
  await act(async () => { await result.current.loadRestore(); });
  await act(async () => { await result.current.runRestore(); });
  expect(requestConfirm).toHaveBeenCalledWith(expect.objectContaining({ message: t().restoreConfirm("older-backup") }));
  expect(native.restoreSessionByName).toHaveBeenCalledWith("older-backup");
  expect(toast.success).not.toHaveBeenCalled();
  expect(toast.info).toHaveBeenCalledWith(t().restoreIncomplete);
  expect(result.current.restoreMsgs.join(" ")).not.toMatch(/skipped|restore failed|denied/);
});

it("only announces confirmed restore rows as complete", async () => {
  native.restoreSessionByName.mockResolvedValue(["restored PATH entry tools", "PATH entry already present: present"]);
  const { result } = renderHook(() => useMoreRestore(vi.fn()));
  await act(async () => { await result.current.loadRestore(); });
  await act(async () => { await result.current.runRestore(); });
  expect(toast.success).toHaveBeenCalledWith(t().restoreDone);
  expect(result.current.restoreMsgs).toEqual([`${t().restoreDone}: tools`, `${t().restoreAlreadyPresent}: present`]);
});

it("exposes backup loading failures with a retry action instead of the empty vault", async () => {
  native.backupSessions.mockRejectedValueOnce(new Error("failed"));
  const { result } = renderHook(() => useMoreRestore(vi.fn()));
  await act(async () => { await result.current.loadRestore(); });
  const retry = vi.fn();
  render(<RestorePanel sessions={[]} pick="" setPick={() => {}} busy={false} msgs={[]} loadError={result.current.restoreLoadError} onReload={retry} onRun={() => {}} onDelete={() => {}} onClose={() => {}} />);
  expect(screen.getByRole("alert").textContent).toContain(t().restoreLoadFailed);
  expect(screen.queryByText(t().restoreNoSessions)).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: t().manageReload }));
  expect(retry).toHaveBeenCalledOnce();
});

it("shows capped sizes as one floor marker and catches open failures", async () => {
  native.openPath.mockRejectedValueOnce(new Error("failed"));
  const onError = vi.fn();
  render(<DiskRadarPanel onClose={() => {}} onError={onError} />);
  const size = await screen.findByText(/>= 1 KB/);
  expect(size.textContent).not.toMatch(/约|~/);
  fireEvent.click(screen.getByRole("button", { name: t().openLocation }));
  await waitFor(() => expect(onError).toHaveBeenCalledOnce());
});

it("rejects old sibling rows while loading and commits navigation only with the response", async () => {
  native.listTopDirSizes.mockResolvedValue([
    { path: "A", name: "A", size_kb: 1, capped: true },
    { path: "B", name: "B", size_kb: 1, capped: false },
  ]);
  let resolveChild!: (value: unknown[]) => void;
  native.listDirChildren.mockImplementationOnce(() => new Promise(resolve => { resolveChild = resolve; }));
  render(<DiskRadarPanel onClose={() => {}} onError={vi.fn()} />);
  await screen.findByText(/>= 1 KB/);
  const drill = screen.getAllByRole("button", { name: t().detailShow });
  fireEvent.click(drill[0]!);
  fireEvent.click(drill[1]!);
  expect(native.listDirChildren).toHaveBeenCalledTimes(1);
  expect(native.listDirChildren).toHaveBeenCalledWith("A");
  expect((drill[1] as HTMLButtonElement).disabled).toBe(true);
  expect(screen.queryByRole("button", { name: t().navBack })).toBeNull();
  await act(async () => { resolveChild([{ path: "A/child", name: "A child", size_kb: 10, capped: false }]); });
  expect(screen.getByText("A child")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: t().navBack }));
  await waitFor(() => expect(native.listTopDirSizes).toHaveBeenCalledTimes(2));
  expect(native.listDirChildren).toHaveBeenCalledTimes(1);
  expect(await screen.findByText(/>= 1 KB/)).toBeTruthy();
});

it("keeps the current parent after a failed drill and disables back while pending", async () => {
  native.listDirChildren.mockResolvedValueOnce([{ path: "A/child", name: "A child", size_kb: 1, capped: false }]);
  const error = vi.fn();
  render(<DiskRadarPanel onClose={() => {}} onError={error} />);
  await screen.findByText(/>= 1 KB/);
  fireEvent.click(screen.getByRole("button", { name: t().detailShow }));
  await screen.findByText("A child");
  let reject!: (error: Error) => void;
  native.listDirChildren.mockImplementationOnce(() => new Promise((_, no) => { reject = no; }));
  fireEvent.click(screen.getByRole("button", { name: t().detailShow }));
  const back = screen.getByRole("button", { name: t().navBack });
  expect((back as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(back);
  expect(native.listTopDirSizes).toHaveBeenCalledTimes(1);
  await act(async () => reject(new Error("read failed")));
  expect(error).toHaveBeenCalledTimes(1);
  expect(screen.getByText("A child")).toBeTruthy();
  fireEvent.click(back);
  await waitFor(() => expect(native.listTopDirSizes).toHaveBeenCalledTimes(2));
  expect(native.listDirChildren).toHaveBeenCalledTimes(2);
});

it("updates the selected backup after deletion and preserves selection when another is deleted", async () => {
  const older = { name: "older-backup", size_kb: 1, created_at: "" };
  const newer = { ...older, name: "newer-backup" };
  native.backupSessions.mockResolvedValueOnce([older, newer]);
  native.deleteBackupSession.mockResolvedValue(undefined);
  const { result } = renderHook(() => useMoreRestore(vi.fn()));
  await act(async () => { await result.current.loadRestore(); });
  native.backupSessions.mockResolvedValueOnce([newer]);
  await act(async () => { await result.current.deleteSession(older.name); });
  expect(native.deleteBackupSession).toHaveBeenCalledWith(older.name);
  expect(result.current.restorePick).toBe(newer.name);
  native.backupSessions.mockResolvedValueOnce([newer]);
  await act(async () => { await result.current.deleteSession("another-backup"); });
  expect(result.current.restorePick).toBe(newer.name);
  native.backupSessions.mockResolvedValueOnce([]);
  await act(async () => { await result.current.deleteSession(newer.name); });
  expect(result.current.restorePick).toBe("");
});

it("preserves backups on deletion cancellation or failure", async () => {
  const error = vi.fn();
  const { result } = renderHook(() => useMoreRestore(error));
  await act(async () => { await result.current.loadRestore(); });
  vi.mocked(requestConfirm).mockResolvedValueOnce(false);
  await act(async () => { await result.current.deleteSession("older-backup"); });
  expect(native.deleteBackupSession).not.toHaveBeenCalled();
  native.deleteBackupSession.mockRejectedValueOnce("denied");
  await act(async () => { await result.current.deleteSession("older-backup"); });
  expect(error).toHaveBeenCalledOnce();
  expect(result.current.sessions).toHaveLength(1);
  expect(result.current.restorePick).toBe("older-backup");
});

it("reports rejected restore IPC and releases busy state so it can be retried", async () => {
  native.restoreSessionByName.mockRejectedValueOnce("denied");
  const { result } = renderHook(() => useMoreRestore(vi.fn()));
  await act(async () => { await result.current.loadRestore(); });
  await act(async () => { await result.current.runRestore(); });
  expect(result.current.restoreBusy).toBe(false);
  expect(result.current.restoreMsgs).toHaveLength(1);
  expect(toast.error).toHaveBeenCalledOnce();
  expect(toast.success).not.toHaveBeenCalled();
  native.restoreSessionByName.mockResolvedValueOnce(["restored file"]);
  await act(async () => { await result.current.runRestore(); });
  expect(toast.success).toHaveBeenCalledOnce();
});

it("discards an old backup preview after selection changes", async () => {
  let finish!: (preview: unknown) => void;
  native.previewRestore.mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
  const { result } = renderHook(() => useMoreRestore(vi.fn()));
  await act(async () => { await result.current.loadRestore(); });
  await act(async () => { result.current.setRestorePick("newer-backup"); });
  await act(async () => { finish({ name: "older-backup", files: 999, entries: [] }); });
  expect(result.current.restorePreview?.name).toBe("newer-backup");
  expect(result.current.previewLoading).toBe(false);
});

it("rejects a failed fresh preview before confirmation or any restore write", async () => {
  const { result } = renderHook(() => useMoreRestore(vi.fn()));
  await act(async () => { await result.current.loadRestore(); });
  native.previewRestore.mockRejectedValueOnce("seal:map_mismatch");
  await act(async () => { await result.current.runRestore(); });
  expect(requestConfirm).not.toHaveBeenCalled();
  expect(native.restoreSessionByName).not.toHaveBeenCalled();
  expect(result.current.restoreBusy).toBe(false);
});

it("refreshes conflict counts before confirmation and locks duplicate restore requests", async () => {
  const { result } = renderHook(() => useMoreRestore(vi.fn()));
  await act(async () => { await result.current.loadRestore(); });
  native.previewRestore.mockResolvedValueOnce({ name: "older-backup", existing: 2, unavailable: 1, entries: [] });
  native.restoreSessionByName.mockResolvedValueOnce(["restored file"]);
  await act(async () => { await Promise.all([result.current.runRestore(), result.current.runRestore()]); });
  expect(requestConfirm).toHaveBeenCalledWith(expect.objectContaining({
    message: [t().restoreConfirm("older-backup"), t().restoreConflict(2), t().restoreUnavailable(1)].join("\n") }));
  expect(native.restoreSessionByName).toHaveBeenCalledOnce();
});
