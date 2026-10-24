// @vitest-environment jsdom
// Shared admin-rights refusal handling: raw-string detection and the
// elevated-restart offer used by the destructive panels.
import { beforeEach, expect, it, vi } from "vitest";
import { needsAdminRights, offerElevatedRestart } from "../lib/elevate";
import { setLang, t } from "../i18n";

const elevateRestart = vi.fn();
const requestConfirmEx = vi.hoisted(() => vi.fn(async () => ({ ok: true, checked: false })));
vi.mock("../lib/api", () => ({
  api: { elevateRestart: (...a: unknown[]) => elevateRestart(...a) },
}));
vi.mock("../lib/confirm", () => ({ requestConfirmEx }));
vi.mock("../lib/toast", () => ({
  toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() },
}));
beforeEach(() => {
  vi.clearAllMocks();
  setLang("zh");
});

it("detects admin-rights refusals regardless of case", () => {
  expect(needsAdminRights("administrator privileges are required for machine-wide records")).toBe(true);
  expect(needsAdminRights("Administrator PRIVILEGES are required to delete firewall rules")).toBe(true);
  expect(needsAdminRights("boom")).toBe(false);
  expect(needsAdminRights("")).toBe(false);
});

it("restarts elevated after consent and states the reason in the prompt", async () => {
  const ok = await offerElevatedRestart(t().firewallNeedAdmin);
  expect(ok).toBe(true);
  expect(requestConfirmEx).toHaveBeenCalledWith(
    expect.objectContaining({
      title: t().elevateAskTitle,
      message: t().firewallNeedAdmin,
      confirmLabel: t().elevateAskOk,
    }),
  );
  expect(elevateRestart).toHaveBeenCalledTimes(1);
});

it("does nothing when the restart prompt is declined", async () => {
  requestConfirmEx.mockResolvedValueOnce({ ok: false, checked: false });
  const ok = await offerElevatedRestart("why");
  expect(ok).toBe(false);
  expect(elevateRestart).not.toHaveBeenCalled();
});

it("surfaces a failed elevation instead of throwing", async () => {
  elevateRestart.mockRejectedValueOnce("denied");
  const { toast } = await import("../lib/toast");
  const ok = await offerElevatedRestart("why");
  expect(ok).toBe(false);
  expect(toast.error).toHaveBeenCalled();
});
