import { describe, expect, it } from "vitest";
import {
  formatError,
  isAccessDeniedError,
  prettyAppName,
  prettyPublisher,
  shortPath,
  sourceLabel,
} from "../lib/format";

const labels = {
  sourceHkcu: "当前用户",
  sourceHklm64: "系统 64 位",
  sourceHklm32: "系统 32 位",
  sourceStore: "Microsoft Store",
};

describe("sourceLabel", () => {
  it("maps technical codes to beginner labels", () => {
    expect(sourceLabel("HKLM64", labels)).toBe("系统 64 位");
    expect(sourceLabel("HKLM32", labels)).toBe("系统 32 位");
    expect(sourceLabel("HKCU", labels)).toBe("当前用户");
    expect(sourceLabel("Store", labels)).toBe("Microsoft Store");
    expect(sourceLabel("Custom", labels)).toBe("Custom");
  });
});

describe("formatError manage codes", () => {
  it("maps access_denied to Chinese admin guidance", () => {
    // default lang is zh
    const msg = formatError("manage:access_denied:WslInstaller");
    expect(msg).toContain("权限不足");
    expect(msg).toContain("WslInstaller");
    expect(msg).not.toContain("open service key");
    expect(isAccessDeniedError("manage:access_denied:WslInstaller")).toBe(true);
  });

  it("maps open_path not_found and raw os error 2", () => {
    expect(formatError("open_path:not_found")).toContain("位置不存在");
    expect(formatError("系统找不到指定的文件。(os error 2)")).toContain("位置不存在");
    expect(formatError("open_path:empty")).toContain("路径为空");
    expect(formatError("open_path:failed:boom")).toContain("资源管理器");
  });

  it("maps protected service", () => {
    const msg = formatError("manage:protected:WinDefend");
    expect(msg).toContain("关键服务");
    expect(msg).toContain("WinDefend");
  });

  it("maps RemovaError IPC code::message form", () => {
    expect(formatError("manage:protected::WinDefend")).toContain("关键服务");
    expect(formatError("manage:protected::WinDefend")).toContain("WinDefend");
    expect(formatError("manage:protected_registry::HKLM\\SOFTWARE\\Evil")).toContain("关键服务");
    expect(formatError("ai:explain::timeout")).toContain("智能解释");
    expect(formatError("backup:failed::disk full")).toBeTruthy();
  });

  it("does not misparse :: outside known IPC code prefixes (F-R6-09)", () => {
    // PACKAGED location strings use `::` but are not RemovaError IPC.
    const packaged = formatError(
      "PACKAGED::HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run::Evil",
    );
    expect(packaged).not.toContain("关键服务");
    expect(packaged).toContain("PACKAGED");
    // Generic `foo: bar :: detail` is not a known code either.
    const weird = formatError("error: something :: detail");
    expect(weird).not.toContain("关键服务");
    expect(weird).toContain("something");
    // Known prefixes still map.
    expect(formatError("manage:bad_name::svc")).toBeTruthy();
  });

  it("maps safety:protected to path protection, not service copy", () => {
    expect(formatError("safety:protected::")).toContain("受保护");
    expect(formatError("safety:protected::")).not.toContain("服务");
    expect(formatError("safety:protected::C:\\Users\\me\\Documents")).toContain("Documents");
    expect(formatError("safety:protected::C:\\Users\\me\\Documents")).not.toContain("关键服务");
  });
});

describe("prettyAppName", () => {
  it("store GUID shortened", () => {
    const out = prettyAppName("5e869a0b1c2d3e4f.a1b2c3d4e5f67890", "Store");
    expect(out).toContain("5e869a0b");
    expect(out).toContain("Store");
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
  it("store certificate DN collapses to signed package label", () => {
    const dn =
      "CN=Adobe Inc., OU=Acrobat, O=Adobe Inc., L=San Jose, S=California, C=US";
    const out = prettyPublisher(dn);
    expect(out.length).toBeLessThan(dn.length);
    expect(out === "Adobe Inc." || out.length < dn.length).toBe(true);
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
