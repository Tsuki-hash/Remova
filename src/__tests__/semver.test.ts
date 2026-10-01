import { describe, expect, it } from "vitest";
import { formatSize } from "../i18n";
import { compareSemver } from "../semver";

describe("compareSemver", () => {
    // raw git tags with a single leading `v` must not compare as 0.0.0.
    it("strips exactly one leading v on both sides", () => {
      expect(compareSemver("v1.3.1", "1.3.0")).toBeGreaterThan(0);
      expect(compareSemver("1.3.0", "v1.3.1")).toBeLessThan(0);
      expect(compareSemver("v1.3.0", "v1.3.0")).toBe(0);
      // A malformed tag ("vv1.3.0" — only one v stripped) parses as 0.0.0.
      expect(compareSemver("vv1.3.0", "1.3.0")).toBeLessThan(0);
    });

    // prerelease suffixes sort BEFORE the same release; uppercase V counts.
    it("orders prereleases before releases", () => {
      expect(compareSemver("1.2.1-beta.1", "1.2.1")).toBeLessThan(0);
      expect(compareSemver("1.2.1", "1.2.1-rc1")).toBeGreaterThan(0);
      expect(compareSemver("V1.2.3", "1.2.3")).toBe(0);
    });

  it("detects newer patch/minor/major", () => {
    expect(compareSemver("1.1.2", "1.1.1")).toBeGreaterThan(0);
    expect(compareSemver("1.2.0", "1.1.9")).toBeGreaterThan(0);
    expect(compareSemver("2.0.0", "1.9.9")).toBeGreaterThan(0);
  });
  it("equal versions", () => {
    expect(compareSemver("1.1.1", "1.1.1")).toBe(0);
  });
  it("older versions", () => {
    expect(compareSemver("1.0.9", "1.1.0")).toBeLessThan(0);
  });
  it("handles missing parts", () => {
    expect(compareSemver("1.1", "1.1.0")).toBe(0);
    expect(compareSemver("1.1.0.1", "1.1.0")).toBeGreaterThan(0);
  });
});

describe("formatSize", () => {
  it("zero", () => {
    expect(formatSize(0)).toBe("—");
  });
  it("KB / MB / GB", () => {
    expect(formatSize(512)).toBe("512 KB");
    expect(formatSize(2048)).toBe("2.0 MB");
    expect(formatSize(3 * 1024 * 1024)).toBe("3.00 GB");
  });
});
