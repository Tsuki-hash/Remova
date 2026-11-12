// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MigratePanel } from "../components/MigratePanel";
import { ConfirmHost } from "../components/ui/ConfirmHost";
import { api } from "../lib/api";
import { settleConfirm } from "../lib/confirm";
import { t } from "../i18n";

vi.mock("../lib/api", () => ({
  api: { migrateStatus: vi.fn(), migrateCutover: vi.fn(), migrateFinish: vi.fn(), migrateRollback: vi.fn() },
}));

afterEach(() => { settleConfirm(false); cleanup(); vi.clearAllMocks(); });

it.each(["cutover", "finish", "rollback"] as const)("returns focus after cancelling the %s confirmation", async (phase) => {
  const L = t();
  const label = phase === "cutover" ? L.migrateCutover : phase === "finish" ? L.migrateFinish : L.migrateRollback;
  vi.mocked(api.migrateStatus).mockResolvedValue(phase === "cutover" ? "copied" : "cutover");
  render(<><MigratePanel initialSource={"C:\\fixture"} onClose={() => {}} /><ConfirmHost /></>);
  fireEvent.change(screen.getByLabelText(L.migrateTarget), { target: { value: "D:\\fixture" } });
  const opener = await screen.findByRole("button", { name: label });
  opener.focus();
  fireEvent.click(opener);
  const dialog = await screen.findByRole("dialog", { description: /fixture/ });
  expect(dialog.textContent).toContain("C:\\fixture");
  expect(dialog.textContent).toContain("D:\\fixture");
  if (phase === "finish") {
    expect(dialog.textContent).toContain(L.migrateFinishConfirm);
    fireEvent.keyDown(dialog, { key: "Escape" });
  } else {
    fireEvent.click(within(dialog).getByRole("button", { name: L.cancel }));
  }
  await waitFor(() => expect(document.activeElement).toBe(opener));
  expect(api.migrateCutover).not.toHaveBeenCalled();
  expect(api.migrateFinish).not.toHaveBeenCalled();
  expect(api.migrateRollback).not.toHaveBeenCalled();
});
