import { describe, expect, it } from "vitest";
import { progressAnnouncement } from "../lib/installUpdate";
import { t } from "../i18n";

const L = t();

describe("progressAnnouncement (update overlay aria-live cadence)", () => {
  it("announces phases without a percent", () => {
    expect(progressAnnouncement({ phase: "checking" }, L)).toBe(L.versionCheck);
    expect(progressAnnouncement({ phase: "installing" }, L)).toBe(L.versionInstalling);
    expect(progressAnnouncement({ phase: "downloading" }, L)).toBe(L.versionDownloading);
  });

  it("changes the announcement in 10% steps, not per chunk", () => {
    const at = (p: number) =>
      progressAnnouncement({ phase: "downloading", percent: p }, L);
    expect(at(0)).toBe(at(5));
    expect(at(9)).toBe(at(0));
    expect(at(10)).not.toBe(at(9));
    expect(at(100)).toContain("100");
    expect(at(7)).not.toBe(`${at(7)}7`);
  });
});
