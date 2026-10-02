// @vitest-environment jsdom
import { useRef } from "react";
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { usePopupMenu } from "../hooks/usePopupMenu";

function Menu({ open, close }: { open: boolean; close: (focus: boolean) => void }) {
  const menuRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  usePopupMenu({ open, menuRef, triggerRef, onClose: close });
  return <><button ref={triggerRef}>trigger</button><div ref={menuRef} role="menu">
    <button role="menuitem">first</button><button role="menuitem">last</button>
  </div><button>outside</button></>;
}
afterEach(cleanup);

it("focuses and roams menu items with wrapping arrows and Home/End", () => {
  render(<Menu open close={vi.fn()} />);
  const first = screen.getByText("first"), last = screen.getByText("last");
  expect(document.activeElement).toBe(first);
  for (const [key, target] of [["ArrowUp", last], ["ArrowDown", first], ["End", last], ["Home", first], ["ArrowDown", last]] as const) {
    fireEvent.keyDown(window, { key });
    expect(document.activeElement).toBe(target);
  }
});

it.each(["Escape", "Tab"])("closes %s with a focus-restoration request", key => {
  const close = vi.fn();
  render(<Menu open close={close} />);
  expect(fireEvent.keyDown(window, { key, cancelable: true })).toBe(false);
  expect(close).toHaveBeenCalledExactlyOnceWith(true);
});

it("ignores inside clicks, closes outside without taking focus, and removes listeners", () => {
  const close = vi.fn();
  const view = render(<Menu open close={close} />);
  fireEvent.mouseDown(screen.getByText("first"));
  fireEvent.mouseDown(screen.getByText("trigger"));
  expect(close).not.toHaveBeenCalled();
  const outside = screen.getByText("outside");
  outside.focus(); fireEvent.mouseDown(outside);
  expect(close).toHaveBeenCalledExactlyOnceWith(false);
  expect(document.activeElement).toBe(outside);
  close.mockClear();
  view.rerender(<Menu open={false} close={close} />);
  fireEvent.keyDown(window, { key: "Escape" }); fireEvent.mouseDown(document.body);
  expect(close).not.toHaveBeenCalled();
  view.rerender(<Menu open close={close} />);
  view.unmount();
  fireEvent.keyDown(window, { key: "Escape" }); fireEvent.mouseDown(document.body);
  expect(close).not.toHaveBeenCalled();
});
