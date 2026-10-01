import { describe, expect, it, vi, beforeEach } from "vitest";
import { checkLatestRelease, openUpdateDownload, RELEASES_URL } from "../lib/updateCheck";

vi.mock("../lib/api", () => ({
  api: {
    checkGithubLatest: vi.fn(),
    onlineUpdateSupported: vi.fn().mockResolvedValue(false),
    openPath: vi.fn(),
  },
}));

const { api } = await import("../lib/api");

describe("updateCheck", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("maps a version payload to UpdateInfo", async () => {
    (api.checkGithubLatest as ReturnType<typeof vi.fn>).mockResolvedValue({
      version: "1.3.0",
      url: "https://example/r",
      download_url: "https://example/d.exe",
    });
    const r = await checkLatestRelease();
    expect(r.ok).toBe(true);
    if (r.ok) {
      expect(r.info?.version).toBe("1.3.0");
      expect(r.info?.downloadUrl).toBe("https://example/d.exe");
    }
  });

  it("returns info:null when no version", async () => {
    (api.checkGithubLatest as ReturnType<typeof vi.fn>).mockResolvedValue(null);
    const r = await checkLatestRelease();
    expect(r).toEqual({ ok: true, info: null });
  });

  it("reports failure reasons instead of throwing", async () => {
    (api.checkGithubLatest as ReturnType<typeof vi.fn>).mockRejectedValue("offline");
    const r = await checkLatestRelease();
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.reason).toContain("offline");
  });

  it("prefers downloadUrl when opening update", async () => {
    await openUpdateDownload({
      version: "1.3.0",
      url: RELEASES_URL,
      downloadUrl: "https://example/d.exe",
    });
    expect(api.openPath).toHaveBeenCalledWith("https://example/d.exe");
    await openUpdateDownload({ version: "1.3.0", url: RELEASES_URL });
    expect(api.openPath).toHaveBeenCalledWith(RELEASES_URL);
  });

  it("marks installInApp from the backend capability, failing closed", async () => {
    const supported = api.onlineUpdateSupported as ReturnType<typeof vi.fn>;
    (api.checkGithubLatest as ReturnType<typeof vi.fn>).mockResolvedValue({
      version: "1.3.0",
      url: "https://example/r",
    });
    supported.mockResolvedValue(true);
    const r = await checkLatestRelease();
    expect(r.ok).toBe(true);
    if (r.ok) expect(r.info?.installInApp).toBe(true);
    supported.mockResolvedValue(false);
    const r2 = await checkLatestRelease();
    expect(r2.ok).toBe(true);
    if (r2.ok) expect(r2.info?.installInApp).toBe(false);
    supported.mockRejectedValue(new Error("ipc gone"));
    const r3 = await checkLatestRelease();
    expect(r3.ok).toBe(true);
    if (r3.ok) expect(r3.info?.installInApp).toBe(false);
  });

  it("falls back to the release page when the asset URL cannot be opened", async () => {
    const openPath = api.openPath as ReturnType<typeof vi.fn>;
    openPath.mockImplementation((target: string) =>
      target === "https://example/d.exe"
        ? Promise.reject(new Error("open failed"))
        : Promise.resolve(),
    );
    await openUpdateDownload({
      version: "1.3.0",
      url: RELEASES_URL,
      downloadUrl: "https://example/d.exe",
    });
    expect(openPath).toHaveBeenCalledWith("https://example/d.exe");
    expect(openPath).toHaveBeenLastCalledWith(RELEASES_URL);
  });

  it("propagates failure when no target can be opened", async () => {
    const openPath = api.openPath as ReturnType<typeof vi.fn>;
    openPath.mockRejectedValue(new Error("no shell"));
    await expect(
      openUpdateDownload({ version: "1.3.0", url: RELEASES_URL }),
    ).rejects.toThrow("no shell");
  });
});
