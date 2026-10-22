// @vitest-environment jsdom
import { describe, expect, it, vi, afterEach } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";

// The batch summary is where a pending reboot used to look like a failure and
// offered a retry that could only be refused by the backend gate. It is
// rendered for real here rather than through a mock.
import { BatchSummaryPanel, BatchProgress } from "../components/BatchPanels";
import type { BatchItemResult } from "../components/BatchPanels";

function results(): BatchItemResult[] {
  return [
    { key: "a", name: "Alpha", status: "ok", detail: "deleted 3" },
    { key: "b", name: "Beta", status: "failed", detail: "locked" },
    { key: "c", name: "Gamma", status: "skipped", detail: "" },
    { key: "d", name: "Delta", status: "reboot", detail: "reboot pending", officialDone: true },
  ];
}

describe("BatchSummaryPanel", () => {
  afterEach(() => cleanup());

  it("counts a pending reboot separately from real failures", () => {
    const { container } = render(
      <BatchSummaryPanel results={results()} onRetryFailed={vi.fn()} onDismiss={vi.fn()} />,
    );
    // The summary line reads: ok N · failed N · reboot N · skipped N. One real
    // failure and one pending-reboot row must be reported as 1 and 1 — the
    // reboot row must not be absorbed into the failure count.
    const summary = container.querySelector("span")?.textContent ?? "";
    expect(summary).toContain("失败 1");
    expect(summary).toContain("待重启 1");
    expect(summary).not.toContain("失败 2");
    // The reboot row is labelled as pending, not as a failure.
    expect(screen.getByText("待重启")).toBeTruthy();
    expect(screen.getByText("Delta")).toBeTruthy();
  });

  it("keeps the retry action for genuine failures only", () => {
    const onRetryFailed = vi.fn();
    render(
      <BatchSummaryPanel results={results()} onRetryFailed={onRetryFailed} onDismiss={vi.fn()} />,
    );
    const retry = screen.getByText("重试失败项");
    retry.click();
    expect(onRetryFailed).toHaveBeenCalledTimes(1);
  });

  it("hides the retry action when nothing actually failed", () => {
    const only = results().filter((r) => r.status !== "failed");
    render(
      <BatchSummaryPanel results={only} onRetryFailed={vi.fn()} onDismiss={vi.fn()} />,
    );
    expect(screen.queryByText("重试失败项")).toBeNull();
  });

  it("renders progress for the running app", () => {
    render(<BatchProgress index={2} total={5} current="Beta" />);
    expect(screen.getByText(/2\/5/)).toBeTruthy();
    expect(screen.getByRole("progressbar")).toBeTruthy();
  });
});
