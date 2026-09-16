import { describe, expect, it } from "vitest";
import { formatSize } from "../i18n";
import { compareSemver } from "../semver";

describe("compareSemver", () => {
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
