// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, it } from "vitest";
import { CheckupPanel } from "../components/CheckupPanel";
import { CloseChoiceHost } from "../components/ui/CloseChoiceHost";
import { ConfirmHost } from "../components/ui/ConfirmHost";
import { t } from "../i18n";
import { requestCloseChoice, settleCloseChoice } from "../lib/closeMode";
import { requestConfirm, settleConfirm } from "../lib/confirm";

afterEach(() => { cleanup(); settleCloseChoice("cancel"); settleConfirm(false); });

it("gives the visible dialog keyboard ownership and restores focus one layer at a time", async () => {
  function Scene() {
    const [open, setOpen] = useState(false);
    return <>
      <ConfirmHost />
      <CloseChoiceHost />
      <button onClick={() => setOpen(true)}>Open overview</button>
      {open && <CheckupPanel stats={{ total: 1, large: 0, recent: 1 }} orphanScanning={false} orphanCount={1}
        onClose={() => setOpen(false)} onOrphanScan={() => {}} onOpenOrphans={() => {}} />}
    </>;
  }
  const L = t();
  render(<Scene />);
  const opener = screen.getByRole("button", { name: "Open overview" });
  opener.focus();
  fireEvent.click(opener);
  const overview = screen.getByRole("dialog", { name: L.checkupTitle });
  const lastOverviewControl = within(overview).getAllByRole("button").at(-1)!;
  lastOverviewControl.focus();

  let confirm!: ReturnType<typeof requestConfirm>;
  let exit!: ReturnType<typeof requestCloseChoice>;
  act(() => { confirm = requestConfirm({ title: "Confirm work" }); });
  const confirmation = screen.getByRole("dialog", { name: "Confirm work" });
  const confirmFocus = document.activeElement;
  act(() => { exit = requestCloseChoice(); });

  const closeChoice = screen.getByRole("dialog", { name: L.closeChoiceTitle });
  expect(overview.hasAttribute("inert")).toBe(true);
  expect(confirmation.hasAttribute("inert")).toBe(true);
  expect(screen.getAllByRole("dialog")).toEqual([closeChoice]);
  const first = within(closeChoice).getByRole("checkbox");
  const last = within(closeChoice).getByRole("button", { name: L.closeModeTray });
  expect(document.activeElement).toBe(first);
  fireEvent.keyDown(first, { key: "Tab", shiftKey: true });
  expect(document.activeElement).toBe(last);
  fireEvent.keyDown(last, { key: "Tab" });
  expect(document.activeElement).toBe(first);
  opener.focus();
  expect(document.activeElement).toBe(first);

  fireEvent.keyDown(first, { key: "Escape" });
  await expect(exit).resolves.toBe("cancel");
  expect(screen.getAllByRole("dialog")).toEqual([confirmation]);
  expect(document.activeElement).toBe(confirmFocus);
  fireEvent.keyDown(confirmation, { key: "Escape", repeat: true });
  expect(screen.getAllByRole("dialog")).toEqual([confirmation]);
  fireEvent.keyDown(confirmation, { key: "Escape" });
  await expect(confirm).resolves.toBe(false);
  expect(screen.getAllByRole("dialog")).toEqual([overview]);
  expect(document.activeElement).toBe(lastOverviewControl);
  fireEvent.keyDown(lastOverviewControl, { key: "Escape" });
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(document.activeElement).toBe(opener);
});
