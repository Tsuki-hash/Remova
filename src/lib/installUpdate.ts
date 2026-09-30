import { check } from "@tauri-apps/plugin-updater";
import { api } from "./api";
import { acquireUpdateSlot } from "./nativeActivity";
import { requestConfirm } from "./confirm";
import { t } from "../i18n";
import type { UpdateInfo } from "./updateCheck";

export type UpdateProgress = { phase: "checking" | "downloading" | "installing"; percent?: number };

export async function installUpdate(
  info: UpdateInfo,
  busyRef: { current: boolean },
  isMonitoring: () => boolean,
  onProgress: (progress: UpdateProgress | null) => void,
): Promise<void> {
  const L = t();
  if (busyRef.current || isMonitoring()) throw new Error(L.taskBusy);
  if (!info.installInApp || !(await api.onlineUpdateSupported())) {
    throw new Error(L.versionManualOnly);
  }
  if (!(await requestConfirm({
    title: `${L.versionInstall} v${info.version}`,
    message: L.versionInstallConfirm,
    confirmLabel: L.versionInstall,
    cancelLabel: L.cancel,
  }))) return;
  // Recheck after the asynchronous confirmation, then hold both task slots.
  if (busyRef.current || isMonitoring()) throw new Error(L.taskBusy);
  const release = acquireUpdateSlot();
  if (!release) throw new Error(L.taskBusy);
  busyRef.current = true;
  let update: Awaited<ReturnType<typeof check>> = null;
  try {
    onProgress({ phase: "checking" });
    update = await check({ target: "windows-x86_64-nsis", timeout: 30000 });
    if (!update) throw new Error(L.versionCheckFailed);
    if (update.version.replace(/^v/, "") !== info.version.replace(/^v/, "")) {
      throw new Error(L.versionChanged);
    }
    let downloaded = 0;
    let total: number | undefined;
    onProgress({ phase: "downloading" });
    // download() verifies the updater signature before install() is allowed.
    await update.download((event) => {
      if (event.event === "Started") total = event.data.contentLength;
      if (event.event === "Progress") downloaded += event.data.chunkLength;
      onProgress({ phase: "downloading", percent: total ? Math.min(100, Math.floor(downloaded * 100 / total)) : undefined });
    }, { timeout: 120000 });
    onProgress({ phase: "installing" });
    // On Windows the plugin launches the installer and exits the application.
    await update.install();
  } finally {
    await update?.close().catch(() => {});
    busyRef.current = false;
    release();
    onProgress(null);
  }
}
