// @vitest-environment jsdom
import { useEffect } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useAiScanNarrative } from "../hooks/useAiScanNarrative";
import { useAiPanelState } from "../hooks/useAiPanelState";
import { ReportPanel } from "../components/ReportPanel";
import { api } from "../lib/api";
import { currentLang, setLang, t } from "../i18n";
import { toast } from "../lib/toast";
import type { CleanupReport, FullCleanupReport, ReportBriefRequest, ScanResult } from "../types";

vi.mock("../lib/api", () => ({ api: { aiSummarizeReport: vi.fn(), aiExplain: vi.fn(async () => []) } }));

function deferred() {
  let resolve!: (value: string | null) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<string | null>((ok, fail) => { resolve = ok; reject = fail; });
  return { promise, resolve, reject };
}
const pending: (ReturnType<typeof deferred> & { language: string; input: ReportBriefRequest })[] = [];
const report: FullCleanupReport = { app_name: "Fixture", dry_run: false, backup_dir: "", uninstall_ok: false,
  uninstall_message: "", deleted: 1, failed: 0, skipped: 0, delayed: 0, aborted: false,
  restore_point_ok: false, restore_point_msg: "", errors: [], item_details: [] };
type Props = { report: FullCleanupReport | CleanupReport | null; enabled: boolean; revision: number; scan?: ScanResult | null };
const chrome = { clearAiSummary: vi.fn(), clearRiskFilter: vi.fn() };
const setAiSummaryNote = vi.fn();
let latest: ReturnType<typeof useAiScanNarrative> & ReturnType<typeof useAiPanelState>;

function Harness(props: Props) {
  const state = useAiPanelState();
  const narrative = useAiScanNarrative({ scan: props.scan ?? null, selected: null, report: props.report,
    aiEnabled: props.enabled, aiBusy: state.aiBusy, aiReportBusy: state.aiReportBusy,
    langVer: props.revision, L: t(), setAiBusy: state.setAiBusy, setAiNotes: state.setAiNotes,
    setAiSummaryNote, setAiReportBusy: state.setAiReportBusy, setAiReportNote: state.setAiReportNote,
    scanUi: chrome, aiActions: state.actions });
  useEffect(() => { latest = { ...state, ...narrative }; });
  return props.report && <ReportPanel report={props.report} aiEnabled={props.enabled}
    aiReportBusy={state.aiReportBusy} aiReportNote={state.aiReportNote} verifyRows={null}
    onDismiss={() => {}} onRegenerate={() => { void narrative.runAiReport(); }} />;
}

beforeEach(() => {
  pending.length = 0;
  vi.clearAllMocks();
  setLang("zh");
  vi.mocked(api.aiSummarizeReport).mockImplementation(input => {
    const request = { ...deferred(), language: currentLang(), input };
    pending.push(request);
    return request.promise;
  });
});
afterEach(() => { cleanup(); toast.clear(); setLang("zh"); });

const switches = (["zh", "en"] as const).flatMap(from =>
  (["resolve", "reject"] as const).flatMap(outcome =>
    (["before", "after"] as const).map(order => ({ from, to: from === "zh" ? "en" as const : "zh" as const, outcome, order }))));

it.each(switches)("keeps report ownership on $from to $to when the old request $outcome settles $order the new one", async ({ from, to, outcome, order }) => {
  setLang(from);
  const view = render(<Harness report={report} enabled revision={0} />);
  expect(pending).toHaveLength(1);
  expect(latest.aiReportBusy).toBe(true);
  setLang(to);
  view.rerender(<Harness report={report} enabled revision={1} />);
  expect(pending.map(request => request.language)).toEqual([from, to]);
  expect(screen.getByRole<HTMLButtonElement>("button", { name: t().aiReportBusy }).disabled).toBe(true);
  await act(async () => { await latest.runAiReport(); });
  expect(pending).toHaveLength(2);
  const settleOld = async () => { await act(async () => {
    if (outcome === "resolve") pending[0]!.resolve("old-language result");
    else pending[0]!.reject(new Error("old-language failure"));
  }); };
  if (order === "before") {
    await settleOld();
    expect(latest.aiReportBusy).toBe(true);
    expect(latest.aiReportNote).toBeNull();
  }
  await act(async () => { pending[1]!.resolve("current-language result"); });
  if (order === "after") await settleOld();
  expect(latest.aiReportBusy).toBe(false);
  expect(latest.aiReportNote).toBe("current-language result");
  expect(screen.getByText("current-language result")).toBeTruthy();
  expect(screen.queryByText("old-language result")).toBeNull();
  expect(screen.getByRole<HTMLButtonElement>("button", { name: t().aiReportSummary }).disabled).toBe(false);
  expect(pending).toHaveLength(2);
});

it("keeps only the newest report across repeated language switches and same-turn manual retries", async () => {
  const view = render(<Harness report={report} enabled revision={0} />);
  for (const [revision, lang] of [[1, "en"], [2, "zh"], [3, "en"]] as const) {
    setLang(lang);
    view.rerender(<Harness report={report} enabled revision={revision} />);
    act(() => { void latest.runAiReport(); });
    expect(pending).toHaveLength(revision + 1);
  }
  await act(async () => { pending[3]!.resolve("newest"); });
  await act(async () => { pending[2]!.reject(new Error("superseded")); pending[1]!.resolve("old en"); pending[0]!.resolve("old zh"); });
  expect(latest.aiReportNote).toBe("newest");
  expect(latest.aiReportBusy).toBe(false);
  act(() => { void latest.runAiReport(); void latest.runAiReport(); });
  expect(pending).toHaveLength(5);
  expect(latest.aiReportBusy).toBe(true);
  await act(async () => { pending[4]!.resolve("manual"); });
  expect(latest.aiReportNote).toBe("manual");
  expect(latest.aiReportBusy).toBe(false);
});

it.each(["resolve", "reject"] as const)("retires a report on disable and restarts once on enable after old %s", async outcome => {
  const view = render(<Harness report={report} enabled revision={0} />);
  view.rerender(<Harness report={report} enabled={false} revision={0} />);
  expect(latest.aiReportBusy).toBe(false);
  expect(latest.aiReportNote).toBeNull();
  setLang("en");
  view.rerender(<Harness report={report} enabled={false} revision={1} />);
  await act(async () => { await latest.runAiReport(); });
  expect(pending).toHaveLength(1);
  view.rerender(<Harness report={report} enabled revision={1} />);
  expect(pending.map(request => request.language)).toEqual(["zh", "en"]);
  await act(async () => {
    if (outcome === "resolve") pending[0]!.resolve("disabled old result");
    else pending[0]!.reject(new Error("disabled old failure"));
  });
  expect(latest.aiReportBusy).toBe(true);
  expect(latest.aiReportNote).toBeNull();
  await act(async () => { pending[1]!.resolve("enabled"); });
  expect(latest.aiReportNote).toBe("enabled");
  expect(latest.aiReportBusy).toBe(false);
});

it("ties a request to the current report and ignores completion after dismissal", async () => {
  const view = render(<Harness report={report} enabled revision={0} />);
  const other = { ...report, app_name: "Other", deleted: 2 };
  view.rerender(<Harness report={other} enabled revision={0} />);
  expect(pending.map(request => [request.input.appName, request.input.deleted])).toEqual([["Fixture", 1], ["Other", 2]]);
  await act(async () => { pending[0]!.resolve("wrong report"); });
  expect(latest.aiReportBusy).toBe(true);
  expect(latest.aiReportNote).toBeNull();
  view.rerender(<Harness report={null} enabled revision={0} />);
  await act(async () => { pending[1]!.resolve("dismissed report"); await latest.runAiReport(); });
  expect(latest.aiReportBusy).toBe(false);
  expect(latest.aiReportNote).toBeNull();
  expect(pending).toHaveLength(2);
});

it.each(["resolve", "reject"] as const)("finishes the same-language request on %s and allows one manual regeneration", async outcome => {
  render(<Harness report={report} enabled revision={0} />);
  await act(async () => {
    if (outcome === "resolve") pending[0]!.resolve("initial");
    else pending[0]!.reject(new Error("provider failed"));
  });
  expect(latest.aiReportBusy).toBe(false);
  const button = screen.getByRole<HTMLButtonElement>("button", { name: t().aiReportSummary });
  expect(button.disabled).toBe(false);
  fireEvent.click(button);
  fireEvent.click(button);
  expect(pending).toHaveLength(2);
  await act(async () => { pending[1]!.resolve("retry"); });
  expect(latest.aiReportBusy).toBe(false);
  expect(screen.getByText("retry")).toBeTruthy();
});

it("does not retire an unchanged report when the residual scan changes", async () => {
  const view = render(<Harness report={report} enabled revision={0} />);
  const scan: ScanResult = { app_name: "Fixture", items: [{ path: "C:\\Apps\\Fresh", kind: "dir",
    score: 90, confidence: "confirmed", risk: "low", reason: "", evidence: [] }] };
  view.rerender(<Harness report={report} enabled revision={0} scan={scan} />);
  await act(async () => { pending[0]!.resolve("same report"); });
  expect(pending).toHaveLength(1);
  expect(api.aiExplain).toHaveBeenCalledOnce();
  expect(latest.aiReportBusy).toBe(false);
  expect(latest.aiReportNote).toBe("same report");
});

it("does not request an AI summary for a preview", async () => {
  const preview: CleanupReport = { app_name: "Fixture", dry_run: true, uninstall_command: [],
    uninstall_message: "", deleted_planned: 1, skipped: 0, errors: [], item_details: [] };
  render(<Harness report={preview} enabled revision={0} />);
  await act(async () => { await latest.runAiReport(); });
  expect(pending).toHaveLength(0);
  expect(latest.aiReportBusy).toBe(false);
});

it("releases a failed current-language request and lets the report button retry", async () => {
  const view = render(<Harness report={report} enabled revision={0} />);
  setLang("en");
  view.rerender(<Harness report={report} enabled revision={1} />);
  await act(async () => { pending[1]!.reject(new Error("current provider failed")); });
  expect(latest.aiReportBusy).toBe(false);
  expect(latest.aiReportNote).toBeNull();
  const retry = screen.getByRole<HTMLButtonElement>("button", { name: t().aiReportSummary });
  expect(retry.disabled).toBe(false);
  fireEvent.click(retry);
  expect(pending).toHaveLength(3);
  await act(async () => { pending[0]!.resolve("obsolete"); });
  expect(latest.aiReportBusy).toBe(true);
  expect(latest.aiReportNote).toBeNull();
  await act(async () => { pending[2]!.resolve("retried in English"); });
  expect(latest.aiReportBusy).toBe(false);
  expect(screen.getByText("retried in English")).toBeTruthy();
});
