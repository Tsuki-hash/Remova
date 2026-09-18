// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { CleanupConclusion } from "../components/CleanupConclusion";
import type { CleanupItem, ScanResult } from "../types";

function item(partial: Partial<CleanupItem> = {}): CleanupItem {
  return {
    path: "C:\\Program Files\\Demo",
    kind: "dir",
    score: 90,
    confidence: "confirmed",
    risk: "low",
    reason: "install",
    evidence: [],
    shared: false,
    user_data: false,
    size_kb: 1024,
    ...partial,
  };
}

function scan(items: CleanupItem[]): ScanResult {
  return { app_name: "Demo", items };
}

describe("CleanupConclusion", () => {
  it("renders rule-source summary and fires action callbacks", () => {
    const onCleanSafe = vi.fn();
    const onShowConfirm = vi.fn();
    const onShowKeep = vi.fn();
    const onExplain = vi.fn();
    render(
      <CleanupConclusion
        scan={scan([
          item(),
          item({ path: "C:\\x", confidence: "suspected", risk: "medium", size_kb: undefined }),
          item({ path: "C:\\shared", shared: true, confidence: "suspected" }),
        ])}
        scanning={false}
        aiEnabled={false}
        aiBusy={false}
        aiNote={null}
        onCleanSafe={onCleanSafe}
        onShowConfirm={onShowConfirm}
        onShowKeep={onShowKeep}
        onExplain={onExplain}
      />,
    );
    expect(screen.getByText("本地规则")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /清理建议项（1）/ }));
    expect(onCleanSafe).toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: /需确认（1）/ }));
    expect(onShowConfirm).toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: /为何保留（1）/ }));
    expect(onShowKeep).toHaveBeenCalled();
    cleanup();
  });

  it("hides while scanning", () => {
    const { container } = render(
      <CleanupConclusion
        scan={scan([item()])}
        scanning
        aiEnabled
        aiBusy={false}
        aiNote={null}
        onCleanSafe={() => {}}
        onShowConfirm={() => {}}
        onShowKeep={() => {}}
        onExplain={() => {}}
      />,
    );
    expect(container.textContent).toBe("");
    cleanup();
  });

  it("labels AI source when note present", () => {
    render(
      <CleanupConclusion
        scan={scan([item()])}
        aiEnabled
        aiBusy={false}
        aiNote="建议清理安装目录"
        onCleanSafe={() => {}}
        onShowConfirm={() => {}}
        onShowKeep={() => {}}
        onExplain={() => {}}
      />,
    );
    expect(screen.getByText("AI 解读")).toBeTruthy();
    expect(screen.getByText(/建议清理安装目录/)).toBeTruthy();
    cleanup();
  });
});
