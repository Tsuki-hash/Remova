// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { ConfirmHost } from "../components/ui/ConfirmHost";
import { getConfirm, getConfirmChecked, requestConfirmEx, settleConfirm } from "../lib/confirm";
afterEach(() => { if (getConfirm()) act(() => settleConfirm(false)); cleanup(); });

it("keeps opt-in unchecked and updates the visible consequence with the submitted choice", async () => {
  render(<ConfirmHost />);
  let answer!: ReturnType<typeof requestConfirmEx>;
  act(() => { answer = requestConfirmEx({ title: "Cleanup", checkbox: { label: "Backup",
    defaultChecked: false, status: checked => checked ? "Backup planned; abort on failure" : "No backup; cannot restore" } }); });
  expect(getConfirmChecked()).toBe(false);
  expect(screen.getByRole("status").textContent).toBe("No backup; cannot restore");
  fireEvent.click(screen.getByRole("checkbox"));
  expect(screen.getByRole("status").textContent).toBe("Backup planned; abort on failure");
  expect(getConfirmChecked()).toBe(true);
  act(() => settleConfirm(true));
  await expect(answer).resolves.toEqual({ ok: true, checked: true });
});
