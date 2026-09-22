import { describe, expect, it } from "vitest";
import { looksMicrosoft } from "../lib/manageFilter";

describe("looksMicrosoft", () => {
  it("matches explicit Microsoft tokens", () => {
    expect(
      looksMicrosoft({ name: "Microsoft Edge Update", detail: "", location: "" }),
    ).toBe(true);
    expect(
      looksMicrosoft({ name: "Windows Defender Firewall", detail: "", location: "" }),
    ).toBe(true);
  });

  it("matches system path markers", () => {
    expect(
      looksMicrosoft({
        name: "SomeDriver",
        detail: "",
        location: "C:\\Windows\\System32\\drivers\\foo.sys",
      }),
    ).toBe(true);
  });

  it("does not hide third-party apps that merely say Windows", () => {
    expect(
      looksMicrosoft({
        name: "Windows Terminal Tweaks",
        detail: "Acme tools for Windows",
        location: "C:\\Program Files\\Acme",
      }),
    ).toBe(false);
  });

  it("matches short system prefixes", () => {
    expect(looksMicrosoft({ name: "WpnUserService", detail: "", location: "" })).toBe(true);
    expect(looksMicrosoft({ name: "W32Time", detail: "", location: "" })).toBe(true);
  });
});
