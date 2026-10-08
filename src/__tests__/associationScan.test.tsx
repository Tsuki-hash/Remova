// @vitest-environment jsdom
import { act, renderHook, render, screen, cleanup } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { InstalledApp, ScanResult } from "../types";
const native = vi.hoisted(() => ({ beginAssociationScan: vi.fn(), cancelAssociationScan: vi.fn(async () => true),
  associationScanProgress: vi.fn(async (): Promise<{ stage: string; found: number } | null> => null), analyze: vi.fn() }));
vi.mock("../lib/api", () => ({ api: native }));
import { useAssociationScan } from "../hooks/useAssociationScan";
import { ScanProgressBar } from "../components/ScanProgressBar";
import { setLang, t } from "../i18n";
afterEach(() => { cleanup(); vi.clearAllMocks(); });
const app = { name: "sample" } as InstalledApp;
const complete: ScanResult = { app_name: "sample", items: [] };
describe("association scan cancellation", () => {
  it("waits for cancellation acknowledgement and preserves an already committed result", async () => {
    native.beginAssociationScan.mockResolvedValueOnce(61);
    let acknowledge!: (accepted: boolean) => void;
    native.cancelAssociationScan.mockImplementationOnce(() => new Promise(resolve => { acknowledge = resolve; }));
    let finish!: (scan: ScanResult) => void;
    native.analyze.mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
    const view = renderHook(useAssociationScan);
    let pending!: Promise<ScanResult>;
    let cancelling!: Promise<void>;
    await act(async () => { pending = view.result.current.run(app); });
    await act(async () => { cancelling = view.result.current.cancel(); finish(complete); });
    expect(view.result.current.progress?.status).toBe("stopping");
    await act(async () => { acknowledge(false); await cancelling; expect(await pending).toEqual(complete); });
    expect(view.result.current.progress?.status).toBe("complete");
  });
  it("counts specialty results and rejects late failures after unmount as cancellation", async () => {
    native.beginAssociationScan.mockResolvedValueOnce(51).mockResolvedValueOnce(52);
    const view = renderHook(useAssociationScan);
    const scan = vi.fn(async () => ["a", "b"]);
    await act(async () => { expect(await view.result.current.runItems(scan)).toEqual(["a", "b"]); });
    expect(scan).toHaveBeenCalledWith(51);
    expect(view.result.current.progress?.found).toBe(2);
    let fail!: (error: Error) => void;
    let pending!: Promise<string[]>;
    await act(async () => {
      pending = view.result.current.runItems(() => new Promise<string[]>((_resolve, reject) => { fail = reject; }));
    });
    const rejection = expect(pending).rejects.toThrow("scan:cancelled");
    view.unmount();
    fail(new Error("late native error"));
    await rejection;
    expect(native.cancelAssociationScan).toHaveBeenCalledWith(52);
  });
  it("cancels a pending reservation and never starts its late worker", async () => {
    let reserve!: (id: number) => void;
    native.beginAssociationScan.mockImplementationOnce(() => new Promise<number>(r => { reserve = r; }));
    const { result } = renderHook(useAssociationScan);
    let run!: Promise<ScanResult>;
    await act(async () => { run = result.current.run(app); });
    const rejection = expect(run).rejects.toThrow("scan:cancelled");
    await act(async () => { await result.current.cancel(); reserve(41); await rejection; });
    expect(native.cancelAssociationScan).toHaveBeenCalledWith(41);
    expect(native.analyze).not.toHaveBeenCalled();
    expect(result.current.progress?.status).toBe("cancelled");
  });
  it("rejects cancelled results even if the backend resolves successfully", async () => {
    native.beginAssociationScan.mockResolvedValueOnce(42);
    let finish!: (scan: ScanResult) => void;
    native.analyze.mockImplementationOnce(() => new Promise<ScanResult>(r => { finish = r; }));
    const { result } = renderHook(useAssociationScan);
    let run!: Promise<ScanResult>;
    await act(async () => { run = result.current.run(app); });
    const rejection = expect(run).rejects.toThrow("scan:cancelled");
    await act(async () => { await result.current.cancel(); finish(complete); await rejection; });
    expect(result.current.progress?.status).toBe("cancelled");
    expect(native.cancelAssociationScan).toHaveBeenCalledWith(42);
  });
  it("labels cancellation as incomplete in both languages and never shows a percent", () => {
    for (const lang of ["zh", "en"] as const) {
      setLang(lang);
      render(<ScanProgressBar progress={{ status: "cancelled", stage: "files", found: 0 }} onCancel={async () => {}} />);
      expect(screen.getByRole("status").textContent).toBe(t().scanCancelledIncomplete);
      expect(screen.queryByRole("progressbar")).toBeNull();
      cleanup();
    }
    setLang("zh");
    render(<ScanProgressBar progress={{ status: "running", stage: "files", found: 3 }} onCancel={async () => {}} />);
    expect(screen.getByRole("progressbar").getAttribute("value")).toBeNull();
    expect(screen.getByText(t().scanFound(3))).toBeTruthy();
  });
  it("ignores old polling and scan results after a newer run completes", async () => {
    native.beginAssociationScan.mockResolvedValueOnce(43).mockResolvedValueOnce(44);
    let oldResult!: (scan: ScanResult) => void;
    let oldProgress!: (p: { stage: string; found: number } | null) => void;
    native.analyze.mockImplementationOnce(() => new Promise<ScanResult>(r => { oldResult = r; }))
      .mockResolvedValueOnce(complete);
    native.associationScanProgress.mockImplementationOnce(() => new Promise(r => { oldProgress = r; }));
    const { result } = renderHook(useAssociationScan);
    let old!: Promise<ScanResult>;
    await act(async () => { old = result.current.run(app); });
    const rejection = expect(old).rejects.toThrow("scan:cancelled");
    await act(async () => { await result.current.run(app); });
    await act(async () => { oldProgress({ stage: "files", found: 99 }); oldResult(complete); await rejection; });
    expect(result.current.progress).toEqual({ status: "complete", stage: "finalizing", found: 0 });
    expect(native.cancelAssociationScan).toHaveBeenCalledWith(43);
  });
});
