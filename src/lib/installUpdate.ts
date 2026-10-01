import { check } from "@tauri-apps/plugin-updater";
import { api } from "./api";
import { acquireUpdateSlot, UPDATE_BUSY } from "./nativeActivity";
import { formatError } from "./format";
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
  if (busyRef.current || isMonitoring()) throw new Error(UPDATE_BUSY);
  if (!info.installInApp) throw new Error("update:manual_only_type");
  if (!(await api.onlineUpdateSupported())) throw new Error("update:manual_only_env");
  if (!(await requestConfirm({
    title: `${L.versionInstall} v${info.version}`,
    message: L.versionInstallConfirm,
    confirmLabel: L.versionInstallClose,
    cancelLabel: L.cancel,
  }))) return;
  // Recheck after the asynchronous confirmation, then hold both task slots.
  if (busyRef.current || isMonitoring()) throw new Error(UPDATE_BUSY);
  const release = acquireUpdateSlot();
  if (!release) throw new Error(UPDATE_BUSY);
  busyRef.current = true;
  let update: Awaited<ReturnType<typeof check>> = null;
  try {
    onProgress({ phase: "checking" });
    update = await check({ target: "windows-x86_64-nsis", timeout: 30000 });
    if (!update) throw new Error("update:check_failed");
    if (update.version.replace(/^v/, "") !== info.version.replace(/^v/, "")) {
      throw new Error("update:version_changed");
    }
    let downloaded = 0;
    let total: number | undefined;
    onProgress({ phase: "downloading" });
    // download() verifies the updater signature before install() is allowed.
    await update.download((event) => {
      if (event.event === "Started") total = event.data.contentLength;
      if (event.event === "Progress") downloaded += event.data.chunkLength;
      onProgress({ phase: "downloading", percent: total ? Math.min(100, Math.floor(downloaded * 100 / total)) : undefined });
    }, { timeout: 600000 });
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

/** User-facing message for an update-flow failure — never leaks internals.
 * `update:*` tokens map through the shared formatter (single token table);
 * anything else (updater-plugin errors, unknowns) collapses to the generic
 * install-failed string. */
export function updateErrorMessage(e: unknown): string {
  const raw = e instanceof Error ? e.message : typeof e === "string" ? e : String(e);
  if (raw.startsWith("update:")) return formatError(raw);
  return t().versionInstallFailed;
}

/** Screen-reader text for the update overlay — changes in 10% steps so the
 * aria-live region does not announce every download chunk. */
export function progressAnnouncement(
  progress: UpdateProgress,
  L: { versionCheck: string; versionDownloading: string; versionInstalling: string },
): string {
  if (progress.phase === "downloading") {
    if (progress.percent === undefined) return L.versionDownloading;
    const step = Math.min(100, Math.floor(progress.percent / 10) * 10);
    return `${L.versionDownloading} ${step}%`;
  }
  return progress.phase === "checking" ? L.versionCheck : L.versionInstalling;
}
