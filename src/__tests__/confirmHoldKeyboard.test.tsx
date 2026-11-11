// @vitest-environment jsdom
// the dangerous confirm is a 600 ms hold, which is silently inert for a
// keyboard user who taps Enter/Space. The keyboard path must work and must say
// what happened when the hold is released early.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { ConfirmHost } from "../components/ui/ConfirmHost";
import { getConfirm, requestConfirmEx, settleConfirm } from "../lib/confirm";
import { setLang, t } from "../i18n";

afterEach(() => { if (getConfirm()) act(() => settleConfirm(false)); cleanup(); setLang("zh"); });

const danger = () => {
  let answer!: ReturnType<typeof requestConfirmEx>;
  act(() => { answer = requestConfirmEx({ title: "Cleanup", message: "M", danger: true, holdMs: 600 }); });
  return answer;
};

it("announces the keyboard hold path on every danger dialog", () => {
  setLang("zh");
  render(<ConfirmHost />);
  danger();
  const status = screen.getByRole("status");
  expect(status.textContent).toBe(t().confirmHoldKeyboard);
  // The button advertises the shortcut so the affordance is discoverable.
  expect(screen.getByRole("button", { name: t().confirmHoldStart }).getAttribute("aria-keyshortcuts"))
    .toBe("Enter Space");
});

it("a keyboard tap is not a silent no-op — it reports the aborted hold", async () => {
  vi.useFakeTimers();
  try {
    setLang("zh");
    render(<ConfirmHost />);
    const answer = danger();
    const hold = screen.getByRole("button", { name: t().confirmHoldStart });
    fireEvent.keyDown(hold, { key: "Enter" });
    // Still holding: the progress label is showing.
    expect(screen.getByText(t().confirmHoldHint)).toBeTruthy();
    fireEvent.keyUp(hold, { key: "Enter" });
    await act(async () => { await Promise.resolve(); });
    expect(screen.getByRole("status").textContent).toBe(t().confirmHoldKeyboardAborted);
    act(() => settleConfirm(false));
    await expect(answer).resolves.toEqual({ ok: false, checked: false });
  } finally { vi.useRealTimers(); }
});

it("holding the key down long enough confirms the dangerous action", async () => {
  setLang("zh");
  render(<ConfirmHost />);
  const answer = danger();
  const hold = screen.getByRole("button", { name: t().confirmHoldStart });
  // `holding` drives a requestAnimationFrame loop; advance the clock well past
  // the 600 ms threshold so the loop completes.
  const raf = vi.spyOn(window, "requestAnimationFrame")
    .mockImplementation((cb: (time: number) => void) => {
      setTimeout(() => cb(performance.now() + 700), 0);
      return 1;
    });
  const caf = vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});
  try {
    fireEvent.keyDown(hold, { key: "Enter" });
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 20)); });
    await expect(answer).resolves.toEqual({ ok: true, checked: false });
  } finally {
    raf.mockRestore();
    caf.mockRestore();
  }
});

it("a non-danger confirm keeps a plain single click", () => {
  setLang("zh");
  render(<ConfirmHost />);
  let answer!: ReturnType<typeof requestConfirmEx>;
  act(() => { answer = requestConfirmEx({ title: "Plain", confirmLabel: "Go" }); });
  // No hold copy, no keyboard hint, no extra status line.
  expect(screen.queryByText(t().confirmHoldStart)).toBeNull();
  expect(screen.queryByText(t().confirmHoldKeyboard)).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Go" }));
  return expect(answer).resolves.toEqual({ ok: true, checked: false });
});

it("leaving the button during a hold aborts it instead of finishing", async () => {
  setLang("zh");
  render(<ConfirmHost />);
  let answer!: ReturnType<typeof requestConfirmEx>;
  act(() => { answer = requestConfirmEx({ title: "Cleanup", message: "M", danger: true, holdMs: 600 }); });
  try {
    const hold = screen.getByRole("button", { name: t().confirmHoldStart });
    const done = vi.fn();
    void answer.then(done);
    fireEvent.keyDown(hold, { key: "Enter" });
    expect(screen.getByText(t().confirmHoldHint)).toBeTruthy();
    // Focus moves to another control (Tab) while the key is still down: the
    // original button never sees keyup, so blur must end the hold.
    fireEvent.blur(hold);
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 30)); });
    expect(screen.queryByText(t().confirmHoldHint)).toBeNull();
    expect(done).not.toHaveBeenCalled();
    // Releasing the key afterwards must not revive the aborted hold.
    fireEvent.keyUp(hold, { key: "Enter" });
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 30)); });
    expect(done).not.toHaveBeenCalled();
  } finally {
    if (getConfirm()) act(() => settleConfirm(false));
  }
});

it("a cancelled pointer ends the hold", () => {
  setLang("zh");
  render(<ConfirmHost />);
  let answer!: ReturnType<typeof requestConfirmEx>;
  act(() => { answer = requestConfirmEx({ title: "Cleanup", message: "M", danger: true, holdMs: 600 }); });
  try {
    const hold = screen.getByRole("button", { name: t().confirmHoldStart });
    fireEvent.pointerDown(hold, { button: 0, isPrimary: true, pointerId: 1 });
    expect(screen.getByText(t().confirmHoldHint)).toBeTruthy();
    fireEvent.pointerCancel(hold, { pointerId: 1 });
    expect(screen.queryByText(t().confirmHoldHint)).toBeNull();
  } finally {
    if (getConfirm()) act(() => settleConfirm(false));
  }
  void answer;
});

it("accepts a hold only from the primary pointer's primary button", async () => {
  render(<ConfirmHost />);
  const answer = danger();
  const hold = screen.getByRole("button", { name: t().confirmHoldStart });
  let tick: FrameRequestCallback | undefined;
  const raf = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
    tick = cb;
    return 1;
  });
  try {
    fireEvent.pointerDown(hold, { button: 2, isPrimary: true, pointerId: 1 });
    fireEvent.pointerDown(hold, { button: 1, isPrimary: true, pointerId: 1 });
    fireEvent.pointerDown(hold, { button: 0, isPrimary: false, pointerId: 2 });
    expect(tick).toBeUndefined();

    fireEvent.pointerDown(hold, { button: 0, isPrimary: true, pointerId: 1 });
    fireEvent.pointerCancel(hold, { pointerId: 2 });
    expect(screen.getByText(t().confirmHoldHint)).toBeTruthy();
    act(() => tick!(performance.now() + 700));
    await expect(answer).resolves.toEqual({ ok: true, checked: false });
  } finally {
    raf.mockRestore();
  }
});

it.each(["keyboard", "pointer"] as const)("requires a fresh %s hold after the request is replaced", async (kind) => {
  render(<ConfirmHost />);
  const options = { title: "Cleanup", danger: true, holdMs: 600 };
  let first!: ReturnType<typeof requestConfirmEx>;
  act(() => { first = requestConfirmEx(options); });
  let tick: FrameRequestCallback | undefined;
  const raf = vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
    tick = cb;
    return 1;
  });
  try {
    const hold = screen.getByRole("button", { name: t().confirmHoldStart });
    if (kind === "keyboard") fireEvent.keyDown(hold, { key: "Enter" });
    else fireEvent.pointerDown(hold, { button: 0, isPrimary: true, pointerId: 1 });
    const oldTick = tick!;
    expect(oldTick).toBeDefined();

    // Fire the old frame in the same event turn as the replacement, before
    // React gets to unmount its button. It must not resolve the new request.
    let second!: ReturnType<typeof requestConfirmEx>;
    act(() => {
      second = requestConfirmEx(options);
      oldTick(performance.now() + 700);
    });
    await expect(first).resolves.toEqual({ ok: false, checked: false });
    const next = screen.getByRole("button", { name: t().confirmHoldStart });
    expect(next).not.toBe(hold);
    tick = undefined;
    fireEvent.keyDown(next, { key: "Enter", repeat: true });
    fireEvent.pointerUp(next, { button: 0, isPrimary: true, pointerId: 1 });
    expect(tick).toBeUndefined();

    if (kind === "keyboard") {
      fireEvent.keyUp(next, { key: "Enter" });
      fireEvent.keyDown(next, { key: " " });
    } else {
      fireEvent.pointerDown(next, { button: 0, isPrimary: true, pointerId: 1 });
    }
    act(() => tick!(performance.now() + 700));
    await expect(second).resolves.toEqual({ ok: true, checked: false });
  } finally {
    raf.mockRestore();
  }
});
