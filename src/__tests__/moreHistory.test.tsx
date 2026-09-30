// @vitest-environment jsdom
import { describe, expect, it, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useMoreHistory } from "../hooks/useMoreHistory";

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

describe("useMoreHistory", () => {
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
});
