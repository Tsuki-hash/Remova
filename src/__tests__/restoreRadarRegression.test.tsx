// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import { useMoreRestore } from "../hooks/useMoreRestore";
import { RestorePanel } from "../components/RestorePanel";
import { DiskRadarPanel } from "../components/DiskRadarPanel";
import { t } from "../i18n";
import { requestConfirm } from "../lib/confirm";
import { toast } from "../lib/toast";

const native = vi.hoisted(() => ({ backupSessions: vi.fn(), deleteBackupSession: vi.fn(), restoreSessionByName: vi.fn(), listLocalDrives: vi.fn(), listTopDirSizes: vi.fn(), listDirChildren: vi.fn(), openPath: vi.fn() }));
vi.mock("../lib/api", () => ({ api: native }));
vi.mock("../lib/confirm", () => ({ requestConfirm: vi.fn(async () => true) }));
vi.mock("../lib/toast", () => ({ toast: { success: vi.fn(), info: vi.fn(), error: vi.fn() } }));
afterEach(cleanup);
beforeEach(() => {
  vi.clearAllMocks();
  native.backupSessions.mockResolvedValue([{ name: "older-backup", size_kb: 1, created_at: "" }]);
  native.listLocalDrives.mockResolvedValue([{ letter: "C", is_system: true, total_gb: 1, free_gb: 1 }]);
  native.listTopDirSizes.mockResolvedValue([{ path: "root", name: "root", size_kb: 1, capped: true }]);
  native.openPath.mockResolvedValue(undefined);
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

it("does not replace the root view with a stale child response after going back", async () => {
  let resolveChild!: (value: unknown[]) => void;
  native.listDirChildren.mockImplementationOnce(() => new Promise(resolve => { resolveChild = resolve; }));
  render(<DiskRadarPanel onClose={() => {}} onError={vi.fn()} />);
  await screen.findByText(/>= 1 KB/);
  fireEvent.click(screen.getByRole("button", { name: t().detailShow }));
  fireEvent.click(screen.getByRole("button", { name: t().navBack }));
  await waitFor(() => expect(native.listTopDirSizes).toHaveBeenCalledTimes(2));
  await act(async () => { resolveChild([{ path: "stale", name: "stale child", size_kb: 10, capped: false }]); });
  expect(screen.queryByText("stale child")).toBeNull();
  expect(screen.getByText(/>= 1 KB/)).toBeTruthy();
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
