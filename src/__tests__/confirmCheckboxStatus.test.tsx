// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { ConfirmHost } from "../components/ui/ConfirmHost";
import { getConfirm, getConfirmChecked, requestConfirmEx, settleConfirm } from "../lib/confirm";
import { setLang, t } from "../i18n";
afterEach(() => { if (getConfirm()) act(() => settleConfirm(false)); cleanup(); setLang("zh"); });

it.each(["zh", "en"] as const)("keeps backup opt-in unchecked and shows its submitted consequence in %s", async lang => {
  setLang(lang);
  render(<ConfirmHost />);
  let answer!: ReturnType<typeof requestConfirmEx>;
  act(() => { answer = requestConfirmEx({ title: "Cleanup", checkbox: { label: "Backup",
    defaultChecked: false, status: t().cleanupBackupStatus } }); });
  expect(getConfirmChecked()).toBe(false);
  expect(screen.getByRole("status").textContent).toBe(t().cleanupBackupStatus(false));
  fireEvent.click(screen.getByRole("checkbox"));
  expect(screen.getByRole("status").textContent).toBe(t().cleanupBackupStatus(true));
  expect(getConfirmChecked()).toBe(true);
  act(() => settleConfirm(true));
  await expect(answer).resolves.toEqual({ ok: true, checked: true });
});
