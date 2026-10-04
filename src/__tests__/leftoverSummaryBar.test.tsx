// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { LeftoverSummaryBar } from "../components/LeftoverSummaryBar";
import { summarizeLeftovers } from "../lib/decision";
import { t } from "../i18n";
import type { CleanupItem } from "../types";

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

afterEach(cleanup);

describe("LeftoverSummaryBar", () => {
  const items = [
    item(),
    item({ path: "C:\\x", kind: "file", confidence: "suspected", risk: "medium", size_kb: undefined }),
    item({ path: "C:\\shared", kind: "registry", shared: true, confidence: "suspected" }),
  ];
  const base = {
    summary: summarizeLeftovers(items),
    scanning: false,
    riskFilter: null as "confirm" | "keep" | null,
    onSelectSafe: vi.fn(),
    onShowConfirm: vi.fn(),
    onShowKeep: vi.fn(),
  };

  it("renders the three decision chips and fires their actions", () => {
    const onSelectSafe = vi.fn();
    const onShowConfirm = vi.fn();
    render(
      <LeftoverSummaryBar
        {...base}
        onSelectSafe={onSelectSafe}
        onShowConfirm={onShowConfirm}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: t().conclusionCleanSafe(1) }));
    expect(onSelectSafe).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: t().conclusionShowConfirm(1) }));
    expect(onShowConfirm).toHaveBeenCalledOnce();
  });

  it("marks the active risk filter and disables empty buckets", () => {
    render(<LeftoverSummaryBar {...base} riskFilter="confirm" onShowKeep={undefined} />);
    expect(
      screen
        .getByRole("button", { name: t().conclusionShowConfirm(1) })
        .getAttribute("aria-pressed"),
    ).toBe("true");
    expect(
      screen.getByRole("button", { name: t().conclusionShowKeep(1) }).hasAttribute("disabled"),
    ).toBe(false);
    const empty = render(
      <LeftoverSummaryBar
        summary={summarizeLeftovers([item()])}
        riskFilter={null}
        onSelectSafe={vi.fn()}
      />,
    );
    expect(
      empty
        .getByRole("button", { name: t().conclusionShowKeep(0) })
        .hasAttribute("disabled"),
    ).toBe(true);
    empty.unmount();
  });

  it("shows the localized composition counts in one chip", () => {
    render(<LeftoverSummaryBar {...base} />);
    expect(screen.getByText(t().kindLabel("dir") + " 1 · " + t().kindLabel("file") + " 1 · " + t().kindLabel("registry") + " 1")).toBeTruthy();
  });
});
