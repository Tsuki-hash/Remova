/** GitHub latest-release check via Rust command (WebView fetch is unreliable). */
import { api } from "./api";

export type UpdateInfo = {
  version: string;
  url: string;
  downloadUrl?: string;
  installInApp?: boolean;
};

export type UpdateCheckResult =
  | { ok: true; info: UpdateInfo | null }
  | { ok: false; reason: string };

export const RELEASES_URL = "https://github.com/Tsuki-hash/Remova/releases";

/** Fetch latest release; always report *why* a check failed (never a silent null). */
export async function checkLatestRelease(): Promise<UpdateCheckResult> {
  try {
    const raw = await api.checkGithubLatest();
    if (!raw?.version) {
      return { ok: true, info: null };
    }
    const installInApp = await api.onlineUpdateSupported().catch(() => false);
    return {
      ok: true,
      info: {
        version: raw.version,
        url: raw.url || RELEASES_URL,
        downloadUrl: raw.download_url || undefined,
        installInApp,
      },
    };
  } catch (e) {
    const reason = typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
    return { ok: false, reason: reason || "unknown error" };
  }
}

/** Open the installer download when available; on failure fall back to the
 * release page (never re-open the same dead download URL). */
export async function openUpdateDownload(info: UpdateInfo): Promise<void> {
  if (info.downloadUrl) {
    try {
      await api.openPath(info.downloadUrl);
      return;
    } catch {
      // fall through to the release page
    }
  }
  await api.openPath(info.url || RELEASES_URL);
}
