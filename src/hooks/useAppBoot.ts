import { useEffect } from "react";
import { api } from "../lib/api";
import { formatError } from "../lib/format";
import { requestConfirm } from "../lib/confirm";
import { toast } from "../lib/toast";
import { t } from "../i18n";
import { compareSemver } from "../semver";
import { checkLatestRelease, type UpdateInfo } from "../lib/updateCheck";
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
    void api.isElevated().then(setAdmin).catch(() => {});
    void api
      .getAiConfig()
      .then((c) => setAiEnabled(c.enabled))
      .catch(() => {});
    void api
      .loadIgnore()
      .then((ig) => {
        setIgnorePub(ig.publishers || []);
        setIgnoreName(ig.names || []);
      })
      .catch(() => {});
    void api
      .diskUsage()
      .then((d) => {
        const drive = (d.drive || localStorage.getItem("remova_disk_drive") || "C:")
          .slice(0, 2)
          .toUpperCase();
        localStorage.setItem("remova_disk_drive", drive);
        setDisk(`${drive} ${d.free_gb.toFixed(1)} / ${d.total_gb.toFixed(0)} GB`);
      })
      .catch(() => {});
    // Silent update check on launch
    void checkLatestRelease()
      .then((info) => {
        if (!info) return;
        if (compareSemver(info.version, __APP_VERSION__) > 0) {
          setUpdateInfo(info);
          toast.info(`${t().versionNew}: v${info.version}`);
        }
      })
      .catch(() => {});
    // Context menu --analyze handoff runs after apps load (see nav-assist hooks)
    // Custom chrome: close → tray (default) or quit; busy still asks first.
    void (async () => {
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        const win = getCurrentWindow();
        await win.onCloseRequested(async (event) => {
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
      } catch {
        // not in tauri
      }
    })();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
}

/** Manual update check from toolbox. */
export async function checkUpdateNow(
  setUpdateInfo: (v: UpdateInfo | null) => void,
  L: { versionCheckFailed: string; versionNew: string; versionUpToDate: (v: string) => string },
) {
  try {
    const info = await checkLatestRelease();
    if (!info) {
      toast.error(L.versionCheckFailed);
      return;
    }
    if (compareSemver(info.version, __APP_VERSION__) > 0) {
      setUpdateInfo(info);
      toast.success(`${L.versionNew}: v${info.version}`);
      window.open(info.url, "_blank");
    } else {
      toast.success(L.versionUpToDate(__APP_VERSION__));
    }
  } catch {
    toast.error(L.versionCheckFailed);
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
