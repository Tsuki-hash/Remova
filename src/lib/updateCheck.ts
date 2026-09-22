/** GitHub latest-release check via Rust command (WebView fetch is unreliable). */
import { api } from "./api";

export type UpdateInfo = {
  version: string;
  url: string;
  downloadUrl?: string;
};

export const RELEASES_URL = "https://github.com/Tsuki-hash/Remova/releases";

export async function checkLatestRelease(timeoutMs = 8000): Promise<UpdateInfo | null> {
  void timeoutMs;
  try {
    const raw = await api.checkGithubLatest();
    if (!raw?.version) return null;
    return {
      version: raw.version,
      url: raw.url || RELEASES_URL,
      downloadUrl: raw.download_url || undefined,
    };
  } catch {
    return null;
  }
}

/** Open the installer download when available; otherwise the release page. */
export async function openUpdateDownload(info: UpdateInfo): Promise<void> {
  const target = info.downloadUrl || info.url;
  await api.openPath(target);
}
