// @vitest-environment jsdom
// M4 (SOP §9.3): lock diagnosis is read-only; "end and retry" is only offered
// for images inside the app's install directory and always asks first.
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { CleanupResultDetails } from "../components/CleanupResultDetails";
import { setLang, t } from "../i18n";
import type { LockingProcess } from "../types";

const diagnoseLockedPath = vi.fn();
const endLockingProcess = vi.fn();
const requestConfirmEx = vi.fn();
vi.mock("../lib/api", () => ({
  api: {
    diagnoseLockedPath: (...a: unknown[]) => diagnoseLockedPath(...a),
    endLockingProcess: (...a: unknown[]) => endLockingProcess(...a),
  },
}));
vi.mock("../lib/confirm", () => ({
  requestConfirmEx: (...a: unknown[]) => requestConfirmEx(...a),
}));

const root = "C:\\Apps\\Demo";
const item = {
  path: "C:\\Apps\\Demo\\cache",
  kind: "dir",
  status: "failed",
  message: "os error 32",
};
const procs: LockingProcess[] = [
  { pid: 123, name: "Demo", path: "C:\\Apps\\Demo\\demo.exe" },
  { pid: 456, name: "Other", path: "C:\\Other\\other.exe" },
];

beforeEach(() => {
  vi.clearAllMocks();
  setLang("zh");
  diagnoseLockedPath.mockResolvedValue(procs);
  requestConfirmEx.mockResolvedValue({ ok: true, checked: false });
  endLockingProcess.mockResolvedValue(undefined);
});
afterEach(cleanup);

it("offers end-and-retry only for images under the install root", async () => {
  const onRetry = vi.fn();
  const view = render(
    <CleanupResultDetails items={[item]} lockRoot={root} onRetry={onRetry} />,
  );
  fireEvent.click(screen.getByRole("button", { name: t().lockDiagnose }));
  await screen.findByText("C:\\Apps\\Demo\\demo.exe");
  expect(view.container.textContent).toContain("Demo (123)");
  expect(view.container.textContent).toContain("Other (456)");
  const buttons = screen.getAllByRole("button", { name: t().lockEndRetry });
  expect(buttons).toHaveLength(1);

  fireEvent.click(buttons[0]!);
  await waitFor(() => expect(requestConfirmEx).toHaveBeenCalledTimes(1));
  await waitFor(() =>
    expect(endLockingProcess).toHaveBeenCalledWith(
      123,
      "C:\\Apps\\Demo\\demo.exe",
      item.path,
    ),
  );
  await waitFor(() => expect(onRetry).toHaveBeenCalledWith([item.path]));
});

it("falls back to a rescan when no direct retry handler is available", async () => {
  const onRescan = vi.fn();
  render(
    <CleanupResultDetails items={[item]} lockRoot={root} onRescan={onRescan} />,
  );
  fireEvent.click(screen.getByRole("button", { name: t().lockDiagnose }));
  await screen.findByText("C:\\Apps\\Demo\\demo.exe");
  fireEvent.click(screen.getByRole("button", { name: t().lockEndRescan }));
  await waitFor(() =>
    expect(endLockingProcess).toHaveBeenCalledWith(123, "C:\\Apps\\Demo\\demo.exe", item.path),
  );
  await waitFor(() => expect(onRescan).toHaveBeenCalledTimes(1));
});

it("authorizes the reported directory when no install root is passed", async () => {
  const dirItem = { ...item, path: "C:\\Apps\\Demo" };
  const onRetry = vi.fn();
  render(<CleanupResultDetails items={[dirItem]} onRetry={onRetry} />);
  fireEvent.click(screen.getByRole("button", { name: t().lockDiagnose }));
  await screen.findByText("C:\\Apps\\Demo\\demo.exe");
  // The failed item is a dir and the cleanup was app-less, so the backend
  // authorized that dir itself. The button must actually reach the IPC.
  const buttons = screen.getAllByRole("button", { name: t().lockEndRetry });
  expect(buttons).toHaveLength(1);
  fireEvent.click(buttons[0]!);
  await waitFor(() =>
    expect(endLockingProcess).toHaveBeenCalledWith(123, "C:\\Apps\\Demo\\demo.exe", dirItem.path),
  );
  await waitFor(() => expect(onRetry).toHaveBeenCalledWith([dirItem.path]));
});

it("uses the failed file's parent directory for app-less cleanups", async () => {
  const fileItem = {
    path: "C:\\Orphans\\Demo\\locked.dll",
    kind: "file",
    status: "failed",
    message: "os error 32",
  };
  diagnoseLockedPath.mockResolvedValueOnce([
    { pid: 789, name: "Demo", path: "C:\\Orphans\\Demo\\demo.exe" },
  ]);
  const onRetry = vi.fn();
  render(<CleanupResultDetails items={[fileItem]} onRetry={onRetry} />);
  fireEvent.click(screen.getByRole("button", { name: t().lockDiagnose }));
  await screen.findByText("C:\\Orphans\\Demo\\demo.exe");
  fireEvent.click(screen.getByRole("button", { name: t().lockEndRetry }));
  await waitFor(() =>
    expect(endLockingProcess).toHaveBeenCalledWith(
      789,
      "C:\\Orphans\\Demo\\demo.exe",
      fileItem.path,
    ),
  );
  await waitFor(() => expect(onRetry).toHaveBeenCalledWith([fileItem.path]));
});

it("surfaces a server refusal without retrying", async () => {
  endLockingProcess.mockRejectedValueOnce(new Error("process image changed"));
  const onRetry = vi.fn();
  const view = render(
    <CleanupResultDetails items={[item]} lockRoot={root} onRetry={onRetry} />,
  );
  fireEvent.click(screen.getByRole("button", { name: t().lockDiagnose }));
  await screen.findByText("C:\\Apps\\Demo\\demo.exe");
  fireEvent.click(screen.getByRole("button", { name: t().lockEndRetry }));
  await waitFor(() => expect(view.container.textContent).toContain(t().lockImageChanged));
  expect(onRetry).not.toHaveBeenCalled();
});
