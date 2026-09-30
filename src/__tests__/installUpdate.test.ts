import { beforeEach, describe, expect, it, vi } from "vitest";
import { installUpdate, updateErrorMessage } from "../lib/installUpdate";
import { acquireUpdateSlot, trackNativeCall } from "../lib/nativeActivity";
import { check } from "@tauri-apps/plugin-updater";
import { api } from "../lib/api";
import { requestConfirm } from "../lib/confirm";

vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));
vi.mock("../lib/api", () => ({ api: { onlineUpdateSupported: vi.fn() } }));
vi.mock("../lib/confirm", () => ({ requestConfirm: vi.fn() }));

const info = { version: "1.3.1", url: "https://github.com/Tsuki-hash/Remova/releases", installInApp: true };
const update = { version: "1.3.1", download: vi.fn(), install: vi.fn(), close: vi.fn() };

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.onlineUpdateSupported).mockResolvedValue(true);
  vi.mocked(requestConfirm).mockResolvedValue(true);
  vi.mocked(check).mockResolvedValue(update as unknown as Awaited<ReturnType<typeof check>>);
  update.download.mockResolvedValue(undefined);
  update.install.mockResolvedValue(undefined);
  update.close.mockResolvedValue(undefined);
});

describe("safe online update", () => {
  it("holds the native slot through verification and installation, then releases it", async () => {
    const busy = { current: false };
    update.download.mockImplementation(async () => {
      expect(busy.current).toBe(true);
      expect(acquireUpdateSlot()).toBeNull();
      await expect(trackNativeCall(async () => "restore")).rejects.toThrow("update:busy");
    });
    await installUpdate(info, busy, () => false, vi.fn());
    expect(check).toHaveBeenCalledWith({ target: "windows-x86_64-nsis", timeout: 30000 });
    expect(update.install).toHaveBeenCalledOnce();
    expect(update.close).toHaveBeenCalledOnce();
    expect(busy.current).toBe(false);
    expect(await trackNativeCall(async () => "available")).toBe("available");
  });

  it("never installs after a signature or download failure and permits retry", async () => {
    update.download.mockRejectedValueOnce(new Error("invalid signature"));
    const busy = { current: false };
    await expect(installUpdate(info, busy, () => false, vi.fn())).rejects.toThrow("invalid signature");
    expect(update.install).not.toHaveBeenCalled();
    expect(update.close).toHaveBeenCalledOnce();
    expect(busy.current).toBe(false);
    await installUpdate(info, busy, () => false, vi.fn());
    expect(update.install).toHaveBeenCalledOnce();
  });

  it("does not check or download when the user cancels", async () => {
    vi.mocked(requestConfirm).mockResolvedValue(false);
    await installUpdate(info, { current: false }, () => false, vi.fn());
    expect(check).not.toHaveBeenCalled();
  });

  it("rechecks tasks after confirmation to prevent a race", async () => {
    const busy = { current: false };
    vi.mocked(requestConfirm).mockImplementation(async () => { busy.current = true; return true; });
    await expect(installUpdate(info, busy, () => false, vi.fn())).rejects.toThrow("update:busy");
    expect(check).not.toHaveBeenCalled();
  });

  it("refuses installation while a native restore or management call is running", async () => {
    let finish!: () => void;
    const pending = trackNativeCall(() => new Promise<void>((resolve) => { finish = resolve; }));
    try {
      await expect(installUpdate(info, { current: false }, () => false, vi.fn())).rejects.toThrow("update:busy");
      expect(check).not.toHaveBeenCalled();
    } finally { finish(); await pending; }
  });

  it("blocks installation during monitoring or for unsupported launches", async () => {
    await expect(installUpdate(info, { current: false }, () => true, vi.fn())).rejects.toThrow("update:busy");
    vi.mocked(api.onlineUpdateSupported).mockResolvedValue(false);
    await expect(installUpdate(info, { current: false }, () => false, vi.fn())).rejects.toThrow("update:manual_only");
    expect(requestConfirm).not.toHaveBeenCalled();
    expect(check).not.toHaveBeenCalled();
  });

  it("does not install a newly published version that the user has not confirmed", async () => {
    vi.mocked(check).mockResolvedValue({ ...update, version: "1.3.2" } as unknown as Awaited<ReturnType<typeof check>>);
    await expect(installUpdate(info, { current: false }, () => false, vi.fn())).rejects.toThrow("update:version_changed");
    expect(update.download).not.toHaveBeenCalled();
    expect(update.install).not.toHaveBeenCalled();
    expect(update.close).toHaveBeenCalledOnce();
  });

  it("maps update failure tokens to text without leaking internals", async () => {
    expect(updateErrorMessage(new Error("update:busy"))).not.toBe("update:busy");
    expect(updateErrorMessage(new Error("update:manual_only"))).toBeTruthy();
    expect(updateErrorMessage(new Error("update:check_failed"))).toBeTruthy();
    expect(updateErrorMessage(new Error("update:version_changed"))).toBeTruthy();
    expect(updateErrorMessage("update:http_failed")).toBeTruthy();
    // Unknown / plugin errors collapse to the generic message — the raw
    // English string must never reach the toast.
    const generic = updateErrorMessage(new Error("download failed: hyper::x"));
    expect(generic).not.toContain("hyper");
    expect(generic).toBe(updateErrorMessage(new Error("anything else")));
  });

  it("releases both slots when launching the installer fails", async () => {
    update.install.mockRejectedValueOnce(new Error("installer launch failed"));
    const busy = { current: false };
    const progress = vi.fn();
    await expect(installUpdate(info, busy, () => false, progress)).rejects.toThrow("installer launch failed");
    expect(busy.current).toBe(false);
    expect(progress).toHaveBeenLastCalledWith(null);
    expect(await trackNativeCall(async () => true)).toBe(true);
  });
});
