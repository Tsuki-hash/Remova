/** GitHub latest-release check (non-blocking, offline-safe). */

export type UpdateInfo = {
  version: string;
  url: string;
};

export const RELEASES_URL = "https://github.com/Tsuki-hash/Remova/releases";

export async function checkLatestRelease(timeoutMs = 8000): Promise<UpdateInfo | null> {
  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), timeoutMs);
  try {
    const r = await fetch(
      "https://api.github.com/repos/Tsuki-hash/Remova/releases/latest",
      {
        headers: { Accept: "application/vnd.github+json" },
        signal: ctrl.signal,
      },
    );
    if (!r.ok) return null;
    const j = (await r.json()) as { tag_name?: string; html_url?: string } | null;
    const tag = j?.tag_name?.replace(/^v/i, "");
    if (!tag) return null;
    return { version: tag, url: j?.html_url || RELEASES_URL };
  } catch {
    return null;
  } finally {
    clearTimeout(timer);
  }
}
