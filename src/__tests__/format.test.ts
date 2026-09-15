import { describe, expect, it } from "vitest";
import { prettyAppName, prettyPublisher, shortPath } from "../lib/format";

describe("prettyAppName", () => {
  it("store GUID shortened", () => {
    expect(prettyAppName("5e869a0b1c2d3e4f.a1b2c3d4e5f67890", "Store")).toContain("Store app");
  });
  it("normal name passthrough", () => {
    expect(prettyAppName("Visual Studio Code", "HKLM64")).toBe("Visual Studio Code");
  });
});

describe("prettyPublisher", () => {
  it("CN= extracts common name", () => {
    expect(prettyPublisher("CN=Foo Bar, O=Acme, C=US")).toBe("Foo Bar");
  });
  it("long name truncated", () => {
    const long = "A".repeat(50);
    expect(prettyPublisher(long).endsWith("…")).toBe(true);
  });
});

describe("shortPath", () => {
  it("short path unchanged", () => {
    const p = "C:\\Temp\\a.txt";
    expect(shortPath(p)).toBe(p);
  });
  it("long path elided middle", () => {
    const p = `C:\\${"x".repeat(80)}\\end.exe`;
    const s = shortPath(p);
    expect(s.includes("…")).toBe(true);
    expect(s.length).toBeLessThan(p.length);
  });
});
