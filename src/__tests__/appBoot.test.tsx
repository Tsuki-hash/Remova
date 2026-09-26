// @vitest-environment jsdom
// REV-QA-02: boot orchestration — app list load, boot race guard, manual
// update-check flows (useAppBoot was previously 0% covered).
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderHook, act } from "@testing-library/react";
import type { InstalledApp } from "../types";

const listApps = vi.fn();
const openPath = vi.fn();
const checkLatestRelease = vi.fn();

vi.mock("../lib/api", () => ({
  api: {
    listApps: (...a: unknown[]) => listApps(...a),
    isElevated: vi.fn().mockResolvedValue(true),
    getAiConfig: vi.fn().mockResolvedValue({ enabled: false }),
    loadIgnore: vi.fn().mockResolvedValue({ publishers: [], names: [] }),
    diskUsage: vi.fn().mockResolvedValue({ free_gb: 10, total_gb: 100, drive: "C:" }),
    openPath: (...a: unknown[]) => openPath(...a),
  },
}));
vi.mock("../lib/updateCheck", () => ({
  RELEASES_URL: "https://example.com/releases",
  checkLatestRelease: (...a: unknown[]) => checkLatestRelease(...a),
}));
vi.mock("../lib/closeMode", () => ({
  consumeQuitIntent: vi.fn().mockReturnValue(false),
  loadCloseMode: vi.fn().mockReturnValue(null),
  resolveCloseAction: vi.fn().mockResolvedValue("tray"),
}));
vi.mock("../lib/confirm", () => ({ requestConfirm: vi.fn() }));
vi.mock("../lib/toast", () => ({
  toast: { info: vi.fn(), error: vi.fn(), success: vi.fn() },
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: vi.fn().mockRejectedValue(new Error("not in tauri")),
}));

import { useAppBoot, checkUpdateNow } from "../hooks/useAppBoot";
import { toast } from "../lib/toast";

function app(): InstalledApp {
  return {
    name: "DemoApp",
    version: "1",
    publisher: "P",
    install_location: "C:\\Program Files\\DemoApp",
    uninstall_string: "",
    quiet_uninstall_string: "",
    source: "HKLM64",
    registry_key: "k",
    estimated_size_kb: 0,
    install_date: "",
    display_icon: "",
  };
}

function bootSpies() {
  return {
    setApps: vi.fn(),
    setLoading: vi.fn(),
    setError: vi.fn(),
    setAdmin: vi.fn(),
    setAiEnabled: vi.fn(),
    setIgnorePub: vi.fn(),
    setIgnoreName: vi.fn(),
    setDisk: vi.fn(),
    setUpdateInfo: vi.fn(),
  };
}

function mountBoot(spies: ReturnType<typeof bootSpies>) {
  return renderHook(() =>
    useAppBoot({
      ...(spies as unknown as Parameters<typeof useAppBoot>[0]),
      busyRef: { current: false },
    }),
  );
}

describe("useAppBoot (REV-QA-02)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Boot's silent update check must no-op unless a test overrides it.
    checkLatestRelease.mockResolvedValue({ ok: false });
    localStorage.clear();
  });

  it("loads the app list on mount and clears loading", async () => {
    listApps.mockResolvedValue([app()]);
    const spies = bootSpies();
    mountBoot(spies);
    await vi.waitFor(() => expect(spies.setApps).toHaveBeenCalledWith([app()]));
    expect(spies.setLoading).toHaveBeenCalledWith(false);
    expect(spies.setAdmin).toHaveBeenCalledWith(true);
    expect(spies.setIgnorePub).toHaveBeenCalledWith([]);
    expect(spies.setDisk).toHaveBeenCalledWith("C: 10.0 / 100 GB");
  });

  it("maps a list failure to a formatted error and still clears loading", async () => {
    listApps.mockRejectedValue("boot:broken");
    const spies = bootSpies();
    mountBoot(spies);
    await vi.waitFor(() => expect(spies.setError).toHaveBeenCalled());
    expect(String(spies.setError.mock.calls[0]![0])).toContain("boot:broken");
    expect(spies.setApps).not.toHaveBeenCalled();
    expect(spies.setLoading).toHaveBeenCalledWith(false);
  });

  it("ignores the list result when unmounted first (boot race)", async () => {
    let resolveList!: (v: InstalledApp[]) => void;
    listApps.mockReturnValue(new Promise((r) => (resolveList = r)));
    const spies = bootSpies();
    const { unmount } = mountBoot(spies);
    unmount();
    await act(async () => {
      resolveList([app()]);
      await Promise.resolve();
    });
    expect(spies.setApps).not.toHaveBeenCalled();
    expect(spies.setLoading).not.toHaveBeenCalled();
    expect(spies.setError).not.toHaveBeenCalled();
  });
});

describe("checkUpdateNow flows", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("surfaces a newer release and opens its download target", async () => {
    checkLatestRelease.mockResolvedValue({
      ok: true,
      info: { version: "99.0.0", downloadUrl: "https://example.com/dl" },
    });
    const setUpdateInfo = vi.fn();
    const L = {
      versionCheckFailed: "检查失败",
      versionNew: "发现新版本",
      versionUpToDate: () => "已是最新",
      openReleasesToast: "去 Releases",
    };
    await act(async () => {
      await checkUpdateNow(setUpdateInfo, L);
    });
    expect(setUpdateInfo).toHaveBeenCalledWith(
      expect.objectContaining({ version: "99.0.0" }),
    );
    expect(toast.success).toHaveBeenCalled();
    expect(openPath).toHaveBeenCalledWith("https://example.com/dl");
  });

  it("keeps quiet when up to date", async () => {
    checkLatestRelease.mockResolvedValue({
      ok: true,
      info: { version: "0.0.1" },
    });
    const setUpdateInfo = vi.fn();
    const L = {
      versionCheckFailed: "检查失败",
      versionNew: "发现新版本",
      versionUpToDate: () => "已是最新",
      openReleasesToast: "去 Releases",
    };
    await act(async () => {
      await checkUpdateNow(setUpdateInfo, L);
    });
    expect(setUpdateInfo).not.toHaveBeenCalled();
    expect(toast.success).toHaveBeenCalledWith("已是最新");
    expect(openPath).not.toHaveBeenCalled();
  });

  it("falls back to the Releases page when the check fails", async () => {
    checkLatestRelease.mockResolvedValue({ ok: false, reason: "offline" });
    const setUpdateInfo = vi.fn();
    const L = {
      versionCheckFailed: "检查失败",
      versionNew: "发现新版本",
      versionUpToDate: () => "已是最新",
      openReleasesToast: "去 Releases",
    };
    await act(async () => {
      await checkUpdateNow(setUpdateInfo, L);
    });
    expect(toast.error).toHaveBeenCalledWith("检查失败: offline");
    expect(openPath).toHaveBeenCalledWith("https://example.com/releases");
  });
});
