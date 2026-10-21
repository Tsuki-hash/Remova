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
vi.mock("../lib/api", () => ({
  api: {
    firewallOrphans: (...a: unknown[]) => firewallOrphans(...a),
    deleteFirewallRules: (...a: unknown[]) => deleteFirewallRules(...a),
  },
}));
vi.mock("../lib/confirm", () => ({ requestConfirmEx: vi.fn(async () => ({ ok: true, checked: false })) }));
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
  firewallOrphans.mockResolvedValue({ rules: [rule], omitted: 0 });
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
  firewallOrphans.mockResolvedValueOnce({ rules: [rule], omitted: 0 }).mockResolvedValueOnce({
    rules: [],
    omitted: 0,
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
