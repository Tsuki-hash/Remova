// @vitest-environment jsdom
// boot orchestration — app list load, boot race guard, manual
// update-check flows (useAppBoot was previously 0% covered).
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderHook, act } from "@testing-library/react";
import type { InstalledApp } from "../types";

const listApps = vi.fn();
const openPath = vi.fn();
const checkLatestRelease = vi.fn();
const openUpdateDownload = vi.fn();

vi.mock("../lib/api", () => ({
  api: {
    listApps: (...a: unknown[]) => listApps(...a),
    isElevated: vi.fn().mockResolvedValue(true),
    getAiConfig: vi.fn().mockResolvedValue({ enabled: false }),
    loadIgnore: vi.fn().mockResolvedValue({ publishers: [], names: [] }),
    diskUsage: vi.fn().mockResolvedValue({ free_gb: 10, total_gb: 100, drive: "C:" }),
    openPath: (...a: unknown[]) => openPath(...a),
    elevateRestart: vi.fn(),
  },
}));
vi.mock("../lib/updateCheck", () => ({
  RELEASES_URL: "https://example.com/releases",
  checkLatestRelease: (...a: unknown[]) => checkLatestRelease(...a),
  openUpdateDownload: (...a: unknown[]) => openUpdateDownload(...a),
}));
vi.mock("../lib/closeMode", async () => ({
  ...await vi.importActual<typeof import("../lib/closeMode")>("../lib/closeMode"),
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
import { formatError } from "../lib/format";
import { trackNativeCall } from "../lib/nativeActivity";
import { requestConfirm } from "../lib/confirm";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "../lib/api";

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

it.each(["restore_session_by_name", "run_full_cleanup"])("guards window and tray exits during %s", async command => {
  vi.clearAllMocks();
  listApps.mockResolvedValue([]);
  checkLatestRelease.mockResolvedValue({ ok: false });
  let handleClose!: (event: { preventDefault: () => void }) => Promise<void>;
  let quit!: () => void;
  const closeOff = vi.fn(), quitOff = vi.fn(), preventDefault = vi.fn();
  const win = {
    onCloseRequested: vi.fn(async (handler: typeof handleClose) => { handleClose = handler; return closeOff; }),
    listen: vi.fn(async (name: string, handler: () => void) => {
      if (name === "remova:request-quit") { quit = handler; return quitOff; }
      return vi.fn();
    }),
    destroy: vi.fn(async () => {}), hide: vi.fn(async () => {}),
    close: vi.fn(async () => { await handleClose({ preventDefault }); }),
  };
  vi.mocked(getCurrentWindow).mockReturnValue(win as unknown as ReturnType<typeof getCurrentWindow>);
  const view = mountBoot(bootSpies());
  let finish!: () => void;
  const task = trackNativeCall(() => new Promise<void>(resolve => { finish = resolve; }), command);
  try {
    await vi.waitFor(() => expect(win.listen).toHaveBeenCalledTimes(2));
    let answer!: (ok: boolean) => void;
    vi.mocked(requestConfirm).mockImplementationOnce(() => new Promise(resolve => { answer = resolve; }));
    const closing = handleClose({ preventDefault });
    await handleClose({ preventDefault });
    expect(requestConfirm).toHaveBeenCalledOnce();
    await act(async () => { answer(false); await closing; });
    expect(preventDefault).toHaveBeenCalled();
    expect(win.destroy).not.toHaveBeenCalled();
    expect(win.hide).not.toHaveBeenCalled();
    vi.mocked(requestConfirm).mockResolvedValueOnce(false);
    await act(async () => { quit(); });
    expect(win.close).toHaveBeenCalledOnce();
    expect(win.destroy).not.toHaveBeenCalled();
    await act(async () => { finish(); await task; });
    await act(async () => { await handleClose({ preventDefault }); });
    expect(win.hide).toHaveBeenCalledOnce();
    expect(win.destroy).not.toHaveBeenCalled();
    const before = vi.mocked(requestConfirm).mock.calls.length;
    await act(async () => { quit(); });
    expect(win.destroy).toHaveBeenCalledOnce();
    expect(requestConfirm).toHaveBeenCalledTimes(before);
    const again = trackNativeCall(() => new Promise<void>(resolve => { finish = resolve; }), command);
    vi.mocked(requestConfirm).mockResolvedValueOnce(true);
    await act(async () => { quit(); });
    expect(win.destroy).toHaveBeenCalledTimes(2);
    finish(); await again;
  } finally {
    finish(); await task;
    view.unmount();
    expect(closeOff).toHaveBeenCalledOnce();
    expect(quitOff).toHaveBeenCalledOnce();
    vi.mocked(getCurrentWindow).mockImplementation(() => { throw new Error("not in tauri"); });
  }
});

it.each(["accept", "cancel", "uac-failure"])("confirms busy elevation before spawning: %s", async outcome => {
  vi.clearAllMocks();
  listApps.mockResolvedValue([]);
  checkLatestRelease.mockResolvedValue({ ok: false });
  let elevate!: () => void;
  let answer!: (ok: boolean) => void;
  const win = {
    onCloseRequested: vi.fn(async () => vi.fn()),
    listen: vi.fn(async (name: string, handler: () => void) => {
      if (name === "remova:request-elevate") elevate = handler;
      return vi.fn();
    }),
    show: vi.fn(async () => {}), unminimize: vi.fn(async () => {}), setFocus: vi.fn(async () => {}),
    destroy: vi.fn(async () => {}),
  };
  vi.mocked(getCurrentWindow).mockReturnValue(win as unknown as ReturnType<typeof getCurrentWindow>);
  vi.mocked(requestConfirm).mockImplementationOnce(() => new Promise(resolve => { answer = resolve; }));
  vi.mocked(api.elevateRestart).mockImplementation(async () => {
    if (outcome === "uac-failure") throw new Error("elevate:cancelled:1223");
  });
  const spies = bootSpies();
  const view = renderHook(() => useAppBoot({ ...spies, busyRef: { current: true } }));
  try {
    await vi.waitFor(() => expect(elevate).toBeTypeOf("function"));
    vi.useFakeTimers();
    await act(async () => { elevate(); });
    await act(async () => { await vi.advanceTimersByTimeAsync(16_000); });
    expect(api.elevateRestart).not.toHaveBeenCalled();
    expect(win.destroy).not.toHaveBeenCalled();
    await act(async () => { answer(outcome !== "cancel"); });
    if (outcome === "cancel") {
      expect(api.elevateRestart).not.toHaveBeenCalled();
      expect(win.destroy).not.toHaveBeenCalled();
    } else {
      expect(api.elevateRestart).toHaveBeenCalledWith(true);
      if (outcome === "accept") {
        // The old window may only destroy itself after the elevated copy has
        // been spawned — destroy-before-spawn would strand the handoff.
        expect(vi.mocked(api.elevateRestart).mock.invocationCallOrder[0]!).toBeLessThan(
          win.destroy.mock.invocationCallOrder[0]!,
        );
      }
      expect(win.destroy).toHaveBeenCalledTimes(outcome === "accept" ? 1 : 0);
      if (outcome === "uac-failure") expect(toast.error).toHaveBeenCalledOnce();
    }
  } finally {
    vi.useRealTimers();
    view.unmount();
    vi.mocked(getCurrentWindow).mockImplementation(() => { throw new Error("not in tauri"); });
  }
});

it("skips the busy confirm and spawns directly when nothing is in flight", async () => {
  vi.clearAllMocks();
  listApps.mockResolvedValue([]);
  checkLatestRelease.mockResolvedValue({ ok: false });
  let elevate!: () => void;
  const win = {
    onCloseRequested: vi.fn(async () => vi.fn()),
    listen: vi.fn(async (name: string, handler: () => void) => {
      if (name === "remova:request-elevate") elevate = handler;
      return vi.fn();
    }),
    show: vi.fn(async () => {}), unminimize: vi.fn(async () => {}), setFocus: vi.fn(async () => {}),
    destroy: vi.fn(async () => {}),
  };
  vi.mocked(getCurrentWindow).mockReturnValue(win as unknown as ReturnType<typeof getCurrentWindow>);
  // Own implementation: earlier tests leave a throwing elevateRestart behind.
  vi.mocked(api.elevateRestart).mockImplementation(async () => {});
  const spies = bootSpies();
  const view = renderHook(() => useAppBoot({ ...spies, busyRef: { current: false } }));
  try {
    await vi.waitFor(() => expect(elevate).toBeTypeOf("function"));
    await act(async () => { elevate(); });
    expect(requestConfirm).not.toHaveBeenCalled();
    expect(api.elevateRestart).toHaveBeenCalledWith(true);
    await vi.waitFor(() => expect(win.destroy).toHaveBeenCalledOnce());
  } finally {
    view.unmount();
    vi.mocked(getCurrentWindow).mockImplementation(() => { throw new Error("not in tauri"); });
  }
});

describe("useAppBoot", () => {
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

  it("late side-channel boot responses after unmount do not touch state", async () => {
    const { api } = await import("../lib/api");
    let resolveElevated!: (v: boolean) => void;
    vi.mocked(api.isElevated).mockReturnValue(
      new Promise<boolean>((r) => (resolveElevated = r)),
    );
    const spies = bootSpies();
    const { unmount } = mountBoot(spies);
    await vi.waitFor(() => expect(spies.setApps).toHaveBeenCalled());
    unmount();
    await act(async () => {
      resolveElevated(true);
      await Promise.resolve();
    });
    expect(spies.setAdmin).not.toHaveBeenCalled();
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
      versionNoRelease: "还没有已发布版本",
      versionNew: "发现新版本",
      versionUpToDate: () => "已是最新",
      versionDownloadOpenFailed: "无法打开浏览器",
    };
    await act(async () => {
      await checkUpdateNow(setUpdateInfo, L);
    });
    expect(setUpdateInfo).toHaveBeenCalledWith(
      expect.objectContaining({ version: "99.0.0" }),
    );
    expect(toast.success).toHaveBeenCalled();
    expect(openUpdateDownload).toHaveBeenCalledWith(
      expect.objectContaining({ version: "99.0.0", downloadUrl: "https://example.com/dl" }),
    );
  });

  it("keeps in-app installs inside the app instead of opening a browser", async () => {
    checkLatestRelease.mockResolvedValue({
      ok: true,
      info: { version: "99.0.0", downloadUrl: "https://example.com/dl", installInApp: true },
    });
    const setUpdateInfo = vi.fn();
    const L = {
      versionCheckFailed: "检查失败",
      versionNoRelease: "还没有已发布版本",
      versionNew: "发现新版本",
      versionUpToDate: () => "已是最新",
      versionDownloadOpenFailed: "无法打开浏览器",
    };
    await act(async () => {
      await checkUpdateNow(setUpdateInfo, L);
    });
    expect(setUpdateInfo).toHaveBeenCalled();
    expect(openUpdateDownload).not.toHaveBeenCalled();
  });

  it("keeps quiet when up to date", async () => {
    checkLatestRelease.mockResolvedValue({
      ok: true,
      info: { version: "0.0.1" },
    });
    const setUpdateInfo = vi.fn();
    const L = {
      versionCheckFailed: "检查失败",
      versionNoRelease: "还没有已发布版本",
      versionNew: "发现新版本",
      versionUpToDate: () => "已是最新",
      versionDownloadOpenFailed: "无法打开浏览器",
    };
    await act(async () => {
      await checkUpdateNow(setUpdateInfo, L);
    });
    expect(setUpdateInfo).not.toHaveBeenCalled();
    expect(toast.success).toHaveBeenCalledWith("已是最新");
    expect(openPath).not.toHaveBeenCalled();
  });

  it("reports an unpublished repository as info instead of failure", async () => {
    checkLatestRelease.mockResolvedValue({ ok: true, info: null });
    const setUpdateInfo = vi.fn();
    const L = {
      versionCheckFailed: "检查失败",
      versionNoRelease: "还没有已发布版本",
      versionNew: "发现新版本",
      versionUpToDate: () => "已是最新",
      versionDownloadOpenFailed: "无法打开浏览器",
    };
    await act(async () => {
      await checkUpdateNow(setUpdateInfo, L);
    });
    expect(toast.info).toHaveBeenCalledWith("还没有已发布版本");
    expect(toast.error).not.toHaveBeenCalled();
    expect(setUpdateInfo).not.toHaveBeenCalled();
    expect(openPath).not.toHaveBeenCalled();
  });

  it("falls back to the Releases page when the check fails", async () => {
    checkLatestRelease.mockResolvedValue({ ok: false, reason: "offline" });
    const setUpdateInfo = vi.fn();
    const L = {
      versionCheckFailed: "检查失败",
      versionNoRelease: "还没有已发布版本",
      versionNew: "发现新版本",
      versionUpToDate: () => "已是最新",
      versionDownloadOpenFailed: "无法打开浏览器",
    };
    await act(async () => {
      await checkUpdateNow(setUpdateInfo, L);
    });
    // The reason goes through the shared error formatter — known update:*
    // tokens become localized text instead of leaking internals. A failed
    // check no longer auto-opens the browser.
    expect(toast.error).toHaveBeenCalledWith(formatError("offline"));
    expect(openPath).not.toHaveBeenCalled();
  });

  it("localizes known update tokens instead of leaking the raw reason", async () => {
    checkLatestRelease.mockResolvedValue({ ok: false, reason: "update:http_failed" });
    const setUpdateInfo = vi.fn();
    const L = {
      versionCheckFailed: "检查失败",
      versionNoRelease: "还没有已发布版本",
      versionNew: "发现新版本",
      versionUpToDate: () => "已是最新",
      versionDownloadOpenFailed: "无法打开浏览器",
    };
    await act(async () => {
      await checkUpdateNow(setUpdateInfo, L);
    });
    const shown = String(vi.mocked(toast.error).mock.calls[0]?.[0]);
    expect(shown).not.toContain("update:http_failed");
    expect(openPath).not.toHaveBeenCalled();
  });
});
