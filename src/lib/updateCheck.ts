/** GitHub latest-release check via Rust command (WebView fetch is unreliable). */
import { api } from "./api";

export type UpdateInfo = {
  version: string;
  url: string;
  downloadUrl?: string;
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
    return {
      ok: true,
      info: {
        version: raw.version,
        url: raw.url || RELEASES_URL,
        downloadUrl: raw.download_url || undefined,
      },
    };
  } catch (e) {
    const reason = typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
    return { ok: false, reason: reason || "unknown error" };
  }
}

/** Open the installer download when available; otherwise the release page. */
export async function openUpdateDownload(info: UpdateInfo): Promise<void> {
  const target = info.downloadUrl || info.url;
  await api.openPath(target);
}
