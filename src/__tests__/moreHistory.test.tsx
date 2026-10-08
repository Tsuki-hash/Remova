// @vitest-environment jsdom
import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act, cleanup, render, screen } from "@testing-library/react";
import { useMoreHistory } from "../hooks/useMoreHistory";
import { HistoryPanel } from "../components/HistoryPanel";
import { setLang, t } from "../i18n";

vi.mock("../lib/api", () => ({
  api: {
    history: vi.fn(),
    deleteHistory: vi.fn(),
    clearHistory: vi.fn(),
    exportHistoryCsv: vi.fn(),
  },
}));
vi.mock("../lib/confirm", () => ({
  requestConfirm: vi.fn(),
}));
vi.mock("../lib/toast", () => ({
  toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() },
}));

const { api } = await import("../lib/api");
const { requestConfirm } = await import("../lib/confirm");
const { toast } = await import("../lib/toast");
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

describe("useMoreHistory", () => {
  it("ignores a closed or replaced history read", async () => {
    let finish!: (rows: Awaited<ReturnType<typeof api.history>>) => void;
    vi.mocked(api.history).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
    const error = vi.fn();
    const { result } = renderHook(() => useMoreHistory(error));
    let old!: Promise<void>;
    act(() => { old = result.current.loadHistory(); });
    expect(result.current.historyLoading).toBe(true);
    act(() => result.current.closeHistory());
    vi.mocked(api.history).mockRejectedValueOnce(new Error("current denied"));
    await act(async () => { await result.current.loadHistory(); });
    const currentError = result.current.historyLoadError;
    await act(async () => { finish([]); await old; });
    expect(result.current.historyLoadError).toBe(currentError);
    expect(result.current.openHistory).toBe(false);
    expect(result.current.historyLoading).toBe(false);
    expect(error).toHaveBeenCalledOnce();
  });

  it.each(["zh", "en"] as const)("separates loading, failed and empty history in %s", lang => {
    setLang(lang);
    const L = t();
    const reload = vi.fn();
    const props = { history: [], histQ: "", setHistQ: vi.fn(), onDelete: vi.fn(), onClearAll: vi.fn(), onClose: vi.fn(), onReload: reload };
    const view = render(<HistoryPanel {...props} loading />);
    expect(screen.getByRole("status").textContent).toBe(L.loadingGeneric);
    expect(screen.queryByText(L.noHistory)).toBeNull();
    view.rerender(<HistoryPanel {...props} loadError="read denied" />);
    expect(screen.getByRole("alert").textContent).toContain("read denied");
    expect(screen.queryByText(L.noHistory)).toBeNull();
    act(() => screen.getByRole("button", { name: L.manageReload }).click());
    expect(reload).toHaveBeenCalledOnce();
    view.rerender(<HistoryPanel {...props} />);
    expect(screen.getByText(L.noHistory)).toBeTruthy();
  });
  beforeEach(() => {
    vi.clearAllMocks();
    (requestConfirm as ReturnType<typeof vi.fn>).mockResolvedValue(true);
  });

  it("loads history and opens the panel", async () => {
    (api.history as ReturnType<typeof vi.fn>).mockResolvedValue([
      { id: "L1", app_name: "Demo", deleted: 1, failed: 0, skipped: 0, delayed: 0, aborted: false, dry_run: false, backup_dir: "", created_at: "1" },
    ]);
    const onError = vi.fn();
    const { result } = renderHook(() => useMoreHistory(onError));
    await act(async () => {
      await result.current.loadHistory();
    });
    expect(result.current.openHistory).toBe(true);
    expect(result.current.history).toHaveLength(1);
    expect(onError).not.toHaveBeenCalled();
  });

  it("routes load failures to onError", async () => {
    (api.history as ReturnType<typeof vi.fn>).mockRejectedValue("boom");
    const onError = vi.fn();
    const { result } = renderHook(() => useMoreHistory(onError));
    await act(async () => {
      await result.current.loadHistory();
    });
    expect(onError).toHaveBeenCalled();
    expect(result.current.openHistory).toBe(false);
  });

  it("skips delete when confirm is declined", async () => {
    (requestConfirm as ReturnType<typeof vi.fn>).mockResolvedValue(false);
    (api.history as ReturnType<typeof vi.fn>).mockResolvedValue([]);
    const onError = vi.fn();
    const { result } = renderHook(() => useMoreHistory(onError));
    await act(async () => {
      await result.current.deleteHistory("L1");
    });
    expect(api.deleteHistory).not.toHaveBeenCalled();
    expect(onError).not.toHaveBeenCalled();
  });

  it("reloads after confirmed deletion and retains rows on failure", async () => {
    const rows = [{ id: "L1", app_name: "Demo", deleted: 1, failed: 0, skipped: 0, delayed: 0, aborted: false, dry_run: false, backup_dir: "", created_at: "1" }];
    vi.mocked(api.history).mockResolvedValue(rows);
    const error = vi.fn();
    const { result } = renderHook(() => useMoreHistory(error));
    await act(async () => { await result.current.loadHistory(); });
    vi.mocked(api.deleteHistory).mockRejectedValueOnce(new Error("denied"));
    await act(async () => { await result.current.deleteHistory("L1"); });
    expect(result.current.history).toEqual(rows);
    expect(error).toHaveBeenCalledOnce();
    expect(toast.success).not.toHaveBeenCalled();
    vi.mocked(api.deleteHistory).mockResolvedValueOnce(1);
    vi.mocked(api.history).mockResolvedValueOnce([]);
    await act(async () => { await result.current.deleteHistory("L1"); });
    expect(api.deleteHistory).toHaveBeenCalledWith(["L1"]);
    expect(result.current.history).toEqual([]);
    expect(toast.success).toHaveBeenCalledOnce();
  });

  it("only clears history after confirmation and a successful native write", async () => {
    const rows = [{ id: "L1", app_name: "Demo", deleted: 1, failed: 0, skipped: 0, delayed: 0, aborted: false, dry_run: false, backup_dir: "", created_at: "1" }];
    vi.mocked(api.history).mockResolvedValue(rows);
    const error = vi.fn();
    const { result } = renderHook(() => useMoreHistory(error));
    await act(async () => { await result.current.loadHistory(); });
    vi.mocked(requestConfirm).mockResolvedValueOnce(false);
    await act(async () => { await result.current.clearHistory(); });
    expect(api.clearHistory).not.toHaveBeenCalled();
    vi.mocked(api.clearHistory).mockRejectedValueOnce("denied");
    await act(async () => { await result.current.clearHistory(); });
    expect(result.current.history).toEqual(rows);
    expect(error).toHaveBeenCalledOnce();
    vi.mocked(api.clearHistory).mockResolvedValueOnce(undefined);
    await act(async () => { await result.current.clearHistory(); });
    expect(result.current.history).toEqual([]);
    expect(toast.success).toHaveBeenCalledOnce();
  });

  it("downloads CSV and releases its URL and temporary anchor", async () => {
    vi.mocked(api.exportHistoryCsv).mockResolvedValueOnce("name,count\nDemo,1");
    const create = vi.fn(() => "blob:csv"), revoke = vi.fn();
    vi.stubGlobal("URL", class extends URL { static createObjectURL = create; static revokeObjectURL = revoke; });
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (this: HTMLAnchorElement) {
      expect(this.download).toBe("remova-history.csv");
      expect(this.href).toBe("blob:csv");
      expect(this.isConnected).toBe(true);
    });
    const error = vi.fn();
    const { result } = renderHook(() => useMoreHistory(error));
    await act(async () => { await result.current.exportCsv(); });
    expect(create).toHaveBeenCalledWith(expect.any(Blob));
    expect(click).toHaveBeenCalledOnce();
    expect(revoke).toHaveBeenCalledExactlyOnceWith("blob:csv");
    expect(document.querySelector('a[download="remova-history.csv"]')).toBeNull();
    vi.mocked(api.exportHistoryCsv).mockRejectedValueOnce("denied");
    await act(async () => { await result.current.exportCsv(); });
    expect(error).toHaveBeenCalledOnce();
    expect(click).toHaveBeenCalledOnce();
  });
});
