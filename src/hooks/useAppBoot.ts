import { useEffect } from "react";
import { api } from "../lib/api";
import { formatError } from "../lib/format";
import { requestConfirm } from "../lib/confirm";
import { toast } from "../lib/toast";
import { t } from "../i18n";
import { compareSemver } from "../semver";
import { checkLatestRelease, RELEASES_URL, type UpdateInfo } from "../lib/updateCheck";
import { consumeQuitIntent, loadCloseMode, resolveCloseAction } from "../lib/closeMode";
import type { InstalledApp } from "../types";

declare const __APP_VERSION__: string;

/** Launch bootstrap: list apps, admin/AI/ignore/disk, update check, close-guard. */
export function useAppBoot({
  setApps,
  setLoading,
  setError,
  setAdmin,
  setAiEnabled,
  setIgnorePub,
  setIgnoreName,
  setDisk,
  setUpdateInfo,
  busyRef,
}: {
  setApps: (apps: InstalledApp[]) => void;
  setLoading: (v: boolean) => void;
  setError: (e: string | null) => void;
  setAdmin: (v: boolean) => void;
  setAiEnabled: (v: boolean) => void;
  setIgnorePub: (v: string[]) => void;
  setIgnoreName: (v: string[]) => void;
  setDisk: (v: string) => void;
  setUpdateInfo: (v: UpdateInfo | null) => void;
  busyRef: { current: boolean };
}) {
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const list = await api.listApps();
        if (!cancelled) setApps(list);
      } catch (e) {
        if (!cancelled) setError(formatError(e));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    void api
      .isElevated()
      .then(setAdmin)
      .catch((e) => console.warn("[boot] isElevated", e));
    void api
      .getAiConfig()
      .then((c) => setAiEnabled(c.enabled))
      .catch((e) => console.warn("[boot] getAiConfig", e));
    void api
      .loadIgnore()
      .then((ig) => {
        setIgnorePub(ig.publishers || []);
        setIgnoreName(ig.names || []);
      })
      .catch((e) => console.warn("[boot] loadIgnore", e));
    void api
      .diskUsage()
      .then((d) => {
        const drive = (d.drive || localStorage.getItem("remova_disk_drive") || "C:")
          .slice(0, 2)
          .toUpperCase();
        localStorage.setItem("remova_disk_drive", drive);
        setDisk(`${drive} ${d.free_gb.toFixed(1)} / ${d.total_gb.toFixed(0)} GB`);
      })
      .catch((e) => console.warn("[boot] diskUsage", e));
    // Silent update check on launch (never navigates)
    void checkLatestRelease()
      .then((res) => {
        if (!res.ok || !res.info) return;
        const info = res.info;
        if (compareSemver(info.version, __APP_VERSION__) > 0) {
          setUpdateInfo(info);
          toast.info(`${t().versionNew}: v${info.version}`);
        }
      })
      .catch(() => {});
    // Context menu --analyze handoff runs after apps load (see nav-assist hooks)
    // Custom chrome: close → tray (default) or quit; busy still asks first.
    let unlistenClose: (() => void) | null = null;
    let chromeDisposed = false;
    void (async () => {
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        const win = getCurrentWindow();
        const unlisten = await win.onCloseRequested(async (event) => {
          // Tauri requires preventDefault SYNCHRONOUSLY or the window may
          // close before async policy/dialog runs (X appeared dead after choice).
          event.preventDefault();
          if (busyRef.current) {
            const ok = await requestConfirm({
              title: t().closeConfirmBusy,
              confirmLabel: t().confirmOk,
              cancelLabel: t().cancel,
              danger: true,
            });
            if (!ok) return;
          }
          if (consumeQuitIntent() || loadCloseMode() === "quit") {
            try {
              await win.destroy();
            } catch {
              // ignore
            }
            return;
          }
          const action = await resolveCloseAction();
          if (action === "quit") {
            try {
              await win.destroy();
            } catch {
              // ignore
            }
            return;
          }
          if (action === "tray") {
            try {
              await win.hide();
            } catch {
              // ignore
            }
          }
        });
        // the effect may already be gone when the dynamic import resolved.
        if (chromeDisposed) unlisten();
        else unlistenClose = unlisten;
      } catch {
        // not in tauri
      }
    })();
    return () => {
      cancelled = true;
      chromeDisposed = true;
      unlistenClose?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
}

/** Manual update check from toolbox. */
export async function checkUpdateNow(
  setUpdateInfo: (v: UpdateInfo | null) => void,
  L: {
    versionCheckFailed: string;
    versionNew: string;
    versionUpToDate: (v: string) => string;
    openReleasesToast: string;
  },
) {
  const openReleases = async () => {
    try {
      await api.openPath(RELEASES_URL);
      toast.info(L.openReleasesToast);
    } catch {
      /* keep the error toast as the primary signal */
    }
  };
  try {
    const res = await checkLatestRelease();
    if (!res.ok) {
      // Show the concrete reason first, then jump to Releases as a fallback path.
      toast.error(`${L.versionCheckFailed}: ${res.reason}`);
      await openReleases();
      return;
    }
    const info = res.info;
    if (!info) {
      toast.error(L.versionCheckFailed);
      await openReleases();
      return;
    }
    if (compareSemver(info.version, __APP_VERSION__) > 0) {
      setUpdateInfo(info);
      toast.success(`${L.versionNew}: v${info.version}`);
      const target = info.downloadUrl || info.url;
      try {
        await api.openPath(target);
      } catch {
        toast.error(L.versionCheckFailed);
      }
    } else {
      toast.success(L.versionUpToDate(__APP_VERSION__));
    }
  } catch (e) {
    const reason = e instanceof Error ? e.message : String(e);
    toast.error(`${L.versionCheckFailed}: ${reason}`);
    await openReleases();
  }
}

/** Shell context-menu register/unregister toggle from toolbox. */
export async function toggleShellMenuApi(
  shellMenu: boolean,
  setShellMenu: (v: boolean) => void,
  setError: (e: string | null) => void,
  L: { shellUnregister: string; shellMenuOn: string },
) {
  try {
    if (shellMenu) {
      await api.unregisterContextMenu();
      setShellMenu(false);
      toast.success(L.shellUnregister);
    } else {
      await api.registerContextMenu();
      setShellMenu(true);
      toast.success(L.shellMenuOn);
    }
  } catch (e) {
    setError(formatError(e));
  }
}
