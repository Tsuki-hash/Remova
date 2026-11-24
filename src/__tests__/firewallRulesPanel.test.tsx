// @vitest-environment jsdom
// M3 (SOP §8-1): orphan firewall rules are listed unselected; deleting the
// selected values goes through confirmation and a server-side re-scan.
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { FirewallRulesPanel } from "../components/FirewallRulesPanel";
import { setLang, t } from "../i18n";
import type { FirewallRule } from "../types";

const rule: FirewallRule = {
  value_name: "Vendor-Share-In",
  revision: "first-observed-revision",
  rule_name: "Vendor File Share",
  app_path: "C:\\Program Files\\Vendor\\app.exe",
  direction: "In",
  action: "Allow",
  protocol: "6",
  reason: "firewall_app_missing",
};
const selection = ({ value_name, revision }: FirewallRule) => ({ value_name, revision });

const firewallOrphans = vi.fn();
const deleteFirewallRules = vi.fn();
const isElevated = vi.fn();
const elevateRestart = vi.fn();
const requestConfirmEx = vi.hoisted(() => vi.fn(async () => ({ ok: true, checked: false })));
vi.mock("../lib/api", () => ({
  api: {
    appIconData: vi.fn(async () => null),
    firewallOrphans: (...a: unknown[]) => firewallOrphans(...a),
    deleteFirewallRules: (...a: unknown[]) => deleteFirewallRules(...a),
    isElevated: () => isElevated(),
    elevateRestart: (...a: unknown[]) => elevateRestart(...a),
  },
}));
vi.mock("../lib/confirm", () => ({ requestConfirmEx }));
vi.mock("../lib/toast", () => ({
  toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() },
}));
afterEach(cleanup);
beforeEach(() => {
  vi.clearAllMocks();
  isElevated.mockReset().mockResolvedValue(true);
  setLang("zh");
});

const panel = () => render(<FirewallRulesPanel onClose={vi.fn()} onError={vi.fn()} />);

it("lists an orphan rule with its missing program and keeps it unselected", async () => {
  firewallOrphans.mockResolvedValue({ rules: [rule], omitted: 0, completeness: { status: "complete", roots_scanned: 1, failed_roots: [] } });
  panel();
  await screen.findByText(rule.rule_name);
  const L = t();
  expect(screen.getByText(rule.app_path, { exact: false })).toBeTruthy();
  expect(screen.getByText(L.firewallDirectionIn, { exact: false })).toBeTruthy();
  expect(screen.getByRole("checkbox")).toHaveProperty("checked", false);
  expect(screen.getByRole("button", {
    name: new RegExp(L.firewallDeleteSelected), description: L.firewallHint,
  })).toHaveProperty("disabled", true);
  expect(isElevated).not.toHaveBeenCalled();
});

it("deletes only selected rules after confirmation and rescans", async () => {
  firewallOrphans.mockResolvedValueOnce({ rules: [rule], omitted: 0, completeness: { status: "complete", roots_scanned: 1, failed_roots: [] } }).mockResolvedValueOnce({
    rules: [],
    omitted: 0,
    completeness: { status: "complete", roots_scanned: 1, failed_roots: [] },
  });
  deleteFirewallRules.mockResolvedValue({
    removed: [rule.value_name],
    failed: [],
    backup_dir: "C:\\ProgramData\\Remova\\Backup\\sess",
  });
  panel();
  await screen.findByText(rule.rule_name);
  fireEvent.click(screen.getByRole("checkbox"));
  fireEvent.click(screen.getByRole("button", { name: new RegExp(t().firewallDeleteSelected) }));
  await waitFor(() => expect(deleteFirewallRules).toHaveBeenCalledWith([selection(rule)]));
  expect(requestConfirmEx).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({
    message: t().firewallDeleteConfirm(1), danger: true,
  }));
  expect(elevateRestart).not.toHaveBeenCalled();
  await waitFor(() => expect(firewallOrphans).toHaveBeenCalledTimes(2));
  await screen.findByText(t().firewallEmpty);
});

it("selects all rules at once, clears on a second click, and deletes the batch", async () => {
  const second = { ...rule, value_name: "Vendor-Share-In-2", rule_name: "Vendor File Share 2", revision: "second-observed-revision" };
  firewallOrphans.mockResolvedValueOnce({ rules: [rule, second], omitted: 0, completeness: { status: "complete", roots_scanned: 1, failed_roots: [] } });
  deleteFirewallRules.mockResolvedValue({ removed: [], failed: [], backup_dir: "sess" });
  panel();
  await screen.findByText(rule.rule_name);
  fireEvent.click(screen.getByRole("button", { name: t().orphanSelectAll }));
  expect(screen.getAllByRole("checkbox")).toHaveLength(2);
  for (const box of screen.getAllByRole("checkbox")) {
    expect(box).toHaveProperty("checked", true);
  }
  fireEvent.click(screen.getByRole("button", { name: new RegExp(t().firewallDeleteSelected) }));
  await waitFor(() =>
    expect(deleteFirewallRules).toHaveBeenCalledWith(
      expect.arrayContaining([selection(rule), selection(second)]),
    ),
  );
});

it("clears the selection when the select-all toggle is clicked again", async () => {
  const second = { ...rule, value_name: "Vendor-Share-In-2", rule_name: "Vendor File Share 2" };
  firewallOrphans.mockResolvedValue({ rules: [rule, second], omitted: 0, completeness: { status: "complete", roots_scanned: 1, failed_roots: [] } });
  panel();
  await screen.findByText(rule.rule_name);
  fireEvent.click(screen.getByRole("button", { name: t().orphanSelectAll }));
  fireEvent.click(screen.getByRole("button", { name: t().orphanSelectAll }));
  for (const box of screen.getAllByRole("checkbox")) {
    expect(box).toHaveProperty("checked", false);
  }
});

it.each([false, true])("offers only a restart in standard mode (accepted: %s)", async ok => {
  firewallOrphans.mockResolvedValue({ rules: [rule], omitted: 0, completeness: { status: "complete", roots_scanned: 1, failed_roots: [] } });
  isElevated.mockResolvedValue(false);
  requestConfirmEx.mockResolvedValueOnce({ ok, checked: false });
  panel();
  await screen.findByText(rule.rule_name);
  fireEvent.click(screen.getByRole("checkbox"));
  const remove = screen.getByRole("button", { name: new RegExp(t().firewallDeleteSelected) });
  fireEvent.click(remove);
  await waitFor(() => expect(requestConfirmEx).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({
    message: t().firewallNeedAdmin,
    confirmLabel: t().elevateAskOk,
  })));
  await waitFor(() => expect(remove.getAttribute("aria-disabled")).toBeNull());
  expect(remove).toHaveProperty("disabled", false);
  expect(elevateRestart).toHaveBeenCalledTimes(ok ? 1 : 0);
  expect(deleteFirewallRules).not.toHaveBeenCalled();
  expect(screen.getByRole("checkbox")).toHaveProperty("checked", true);
  expect(firewallOrphans).toHaveBeenCalledOnce();
});

it("does not remove rules when an administrator declines the destructive confirmation", async () => {
  firewallOrphans.mockResolvedValue({ rules: [rule], omitted: 0, completeness: { status: "complete", roots_scanned: 1, failed_roots: [] } });
  requestConfirmEx.mockResolvedValueOnce({ ok: false, checked: false });
  panel();
  await screen.findByText(rule.rule_name);
  fireEvent.click(screen.getByRole("checkbox"));
  const remove = screen.getByRole("button", { name: new RegExp(t().firewallDeleteSelected) });
  fireEvent.click(remove);
  await waitFor(() => expect(requestConfirmEx).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({
    message: t().firewallDeleteConfirm(1), danger: true,
  })));
  await waitFor(() => expect(remove.getAttribute("aria-disabled")).toBeNull());
  expect(deleteFirewallRules).not.toHaveBeenCalled();
  expect(elevateRestart).not.toHaveBeenCalled();
});

it("lets the backend decide when the token check fails and still handles its admin refusal", async () => {
  const { toast } = await import("../lib/toast");
  firewallOrphans.mockResolvedValue({ rules: [rule], omitted: 0, completeness: { status: "complete", roots_scanned: 1, failed_roots: [] } });
  isElevated.mockRejectedValueOnce(new Error("token unavailable"));
  deleteFirewallRules.mockRejectedValue(
    "administrator privileges are required to delete firewall rules",
  );
  panel();
  await screen.findByText(rule.rule_name);
  fireEvent.click(screen.getByRole("checkbox"));
  fireEvent.click(screen.getByRole("button", { name: new RegExp(t().firewallDeleteSelected) }));
  await waitFor(() => expect(deleteFirewallRules).toHaveBeenCalledTimes(1));
  await waitFor(() => expect(requestConfirmEx).toHaveBeenCalledTimes(2));
  expect(requestConfirmEx).toHaveBeenLastCalledWith(
    expect.objectContaining({ message: t().firewallNeedAdmin }),
  );
  await waitFor(() => expect(elevateRestart).toHaveBeenCalledTimes(1));
  expect(toast.error).not.toHaveBeenCalled();
});

it("reports the result of an approved deletion even after leaving the panel", async () => {
  const { toast } = await import("../lib/toast");
  firewallOrphans.mockResolvedValue({ rules: [rule], omitted: 0, completeness: { status: "complete", roots_scanned: 1, failed_roots: [] } });
  let finish!: (result: { removed: string[]; failed: string[]; backup_dir: string }) => void;
  deleteFirewallRules.mockReturnValueOnce(new Promise(resolve => { finish = resolve; }));
  const view = panel();
  await screen.findByText(rule.rule_name);
  fireEvent.click(screen.getByRole("checkbox"));
  fireEvent.click(screen.getByRole("button", { name: new RegExp(t().firewallDeleteSelected) }));
  await waitFor(() => expect(deleteFirewallRules).toHaveBeenCalledOnce());
  view.unmount();
  await act(async () => finish({ removed: [rule.value_name], failed: [], backup_dir: "sess" }));
  expect(toast.success).toHaveBeenCalledWith(t().firewallDeleteOk, { detail: "sess" });
  expect(firewallOrphans).toHaveBeenCalledOnce();
});

it("keeps the failed rules on screen after the list refreshes", async () => {
  firewallOrphans.mockResolvedValue({ rules: [rule], omitted: 0, completeness: { status: "complete", roots_scanned: 1, failed_roots: [] } });
  deleteFirewallRules.mockResolvedValue({
    removed: [],
    failed: [rule.value_name],
    backup_dir: "C:\\ProgramData\\Remova\\Backup\\sess",
  });
  panel();
  await screen.findByText(rule.rule_name);
  fireEvent.click(screen.getByRole("checkbox"));
  fireEvent.click(screen.getByRole("button", { name: new RegExp(t().firewallDeleteSelected) }));
  await waitFor(() => expect(deleteFirewallRules).toHaveBeenCalled());
  // The rescan removed the row, but the failure record must survive it: the
  // toast is only a notification, not the result.
  await waitFor(() => expect(firewallOrphans).toHaveBeenCalledTimes(2));
  expect(screen.getByText(rule.value_name)).toBeTruthy();
  expect(screen.getByText("C:\\ProgramData\\Remova\\Backup\\sess")).toBeTruthy();
});

it("shows a retryable error instead of loading forever when the scan fails", async () => {
  firewallOrphans.mockRejectedValueOnce("backend exploded");
  panel();
  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain("backend exploded");
  expect(screen.queryByText(t().loadingGeneric)).toBeNull();
  // Retrying succeeds and replaces the error with the real list.
  firewallOrphans.mockResolvedValueOnce({ rules: [rule], omitted: 0, completeness: { status: "complete", roots_scanned: 1, failed_roots: [] } });
  const retry = screen.getAllByRole("button", { name: t().manageReload })[0];
  expect(retry).toBeTruthy();
  fireEvent.click(retry!);
  await screen.findByText(rule.rule_name);
  expect(screen.queryByRole("alert")).toBeNull();
});
