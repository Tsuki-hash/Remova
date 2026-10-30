// @vitest-environment jsdom
// M3 (SOP §8-1): orphan firewall rules are listed unselected; deleting the
// selected values goes through confirmation and a server-side re-scan.
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { FirewallRulesPanel } from "../components/FirewallRulesPanel";
import { setLang, t } from "../i18n";
import type { FirewallRule } from "../types";

const rule: FirewallRule = {
  value_name: "Vendor-Share-In",
  rule_name: "Vendor File Share",
  app_path: "C:\\Program Files\\Vendor\\app.exe",
  direction: "In",
  action: "Allow",
  protocol: "6",
  reason: "firewall_app_missing",
};

const firewallOrphans = vi.fn();
const deleteFirewallRules = vi.fn();
const elevateRestart = vi.fn();
const requestConfirmEx = vi.hoisted(() => vi.fn(async () => ({ ok: true, checked: false })));
vi.mock("../lib/api", () => ({
  api: {
    firewallOrphans: (...a: unknown[]) => firewallOrphans(...a),
    deleteFirewallRules: (...a: unknown[]) => deleteFirewallRules(...a),
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
  expect(
    screen.getByRole("button", { name: new RegExp(L.firewallDeleteSelected) }),
  ).toHaveProperty("disabled", true);
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
  await waitFor(() => expect(deleteFirewallRules).toHaveBeenCalledWith([rule.value_name]));
  await waitFor(() => expect(firewallOrphans).toHaveBeenCalledTimes(2));
  await screen.findByText(t().firewallEmpty);
});

it("selects all rules at once, clears on a second click, and deletes the batch", async () => {
  const second = { ...rule, value_name: "Vendor-Share-In-2", rule_name: "Vendor File Share 2" };
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
      expect.arrayContaining([rule.value_name, second.value_name]),
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

it("offers an elevated restart when deletion is refused for admin rights", async () => {
  const { toast } = await import("../lib/toast");
  firewallOrphans.mockResolvedValue({ rules: [rule], omitted: 0, completeness: { status: "complete", roots_scanned: 1, failed_roots: [] } });
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
