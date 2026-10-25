// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import {
  OnboardingCard,
  loadOnboarded,
  markOnboarded,
} from "../components/OnboardingCard";
import { useOnboarding } from "../hooks/useOnboarding";
import { renderHook, act } from "@testing-library/react";

beforeEach(() => {
  localStorage.clear();
  cleanup();
});

describe("onboarding persistence", () => {
  it("is unseen for a fresh profile and persists after dismissal", () => {
    expect(loadOnboarded()).toBe(false);
    markOnboarded();
    expect(loadOnboarded()).toBe(true);
    expect(localStorage.getItem("remova_onboarded_v1")).toBe("seen");
  });

  it("treats disabled storage as already seen so the card never pesters", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("denied");
    });
    expect(loadOnboarded()).toBe(true);
    vi.restoreAllMocks();
  });
});

describe("OnboardingCard", () => {
  it("shows the three principles and dismisses via the button", () => {
    const onClose = vi.fn();
    render(<OnboardingCard onClose={onClose} />);
    expect(screen.getByText(/先检查再执行/)).toBeTruthy();
    expect(screen.getByText(/默认可恢复/)).toBeTruthy();
    expect(screen.getByText(/高风险不预选/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "知道了" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

describe("useOnboarding", () => {
  it("opens for a fresh profile; dismissal persists, reopen is session-only", () => {
    const { result } = renderHook(() => useOnboarding());
    expect(result.current.open).toBe(true);
    act(() => result.current.dismiss());
    expect(result.current.open).toBe(false);
    expect(loadOnboarded()).toBe(true);
    act(() => result.current.reopen());
    expect(result.current.open).toBe(true);
    // Reopen does not clear the persisted flag: next boot stays dismissed.
    expect(loadOnboarded()).toBe(true);
  });

  it("stays closed on boot once dismissed previously", () => {
    markOnboarded();
    const { result } = renderHook(() => useOnboarding());
    expect(result.current.open).toBe(false);
  });
});
