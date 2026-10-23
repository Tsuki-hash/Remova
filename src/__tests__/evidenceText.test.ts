// @vitest-environment jsdom
import { afterEach, expect, it } from "vitest";
import { setLang, t } from "../i18n";
import { evidenceText } from "../lib/evidenceText";
afterEach(() => setLang("zh"));
it.each(["zh", "en"] as const)("localizes association evidence while retaining technical details in %s", lang => {
  setLang(lang);
  const raw = { code: "shell_class", label: "Classes key name matches product", weight: 35, detail: "xiaomi-mimo" };
  expect(evidenceText(raw)).toEqual({ label: lang === "zh" ? "Classes 注册项名称匹配软件" : raw.label, detail: "xiaomi-mimo" });
  expect(evidenceText({ ...raw, code: "run_startup", label: "Startup points at install dir", detail: "C:/应用/app.exe" }))
    .toEqual({ label: lang === "zh" ? "启动项指向安装目录" : "Startup points at install dir", detail: "C:/应用/app.exe" });
  expect(evidenceText({ ...raw, code: "windows_service" }).label)
    .toBe(t().scanEvidenceLabel("windows_service", ""));
  expect(evidenceText({ ...raw, code: "install_monitor", label: "Added during install (likely cache/temp)" }).label)
    .toBe(lang === "zh" ? "安装期间新增（可能是缓存或临时文件）" : "Added during install (likely cache/temp)");
  expect(evidenceText({ ...raw, code: "future_native_code" })).toEqual({ ...raw, code: "future_native_code" });
});
// the specialty scanners emit `tool_cache_shared` and a domain
// label; a missing map entry leaked the raw English code into the row.
it.each(["zh", "en"] as const)("localizes specialty-scan evidence codes in %s", lang => {
  setLang(lang);
  const raw = { code: "tool_cache_shared", label: "dev", weight: 30, detail: "npm-cache\\_cacache" };
  expect(evidenceText(raw).label).toBe(t().scanEvidenceLabel("tool_cache_shared", "dev"));
  expect(evidenceText(raw).label).not.toBe("");
  expect(evidenceText(raw).label).not.toBe("tool_cache_shared");
  for (const code of ["tool_cache", "tool_cache_shared", "updater_cache", "installer_package"]) {
    expect(t().scanEvidenceLabel(code, "")).not.toBe("");
  }
  for (const domain of ["dev", "game", "browser"]) {
    expect(t().scanEvidenceLabel(domain, "")).not.toBe("");
  }
});
it.each(["zh", "en"] as const)("localizes native orphan evidence without guessing paths in %s", lang => {
  setLang(lang);
  const sample = { code: "orphan_no_owner", label: "No matching uninstall entry", weight: 30,
    detail: "folder `Vendor` not matched to installed software" };
  expect(evidenceText(sample)).toEqual({ label: t().orphanEvNoOwner, detail: "" });
  expect(evidenceText({ ...sample, code: "orphan_many_files", detail: "12 files in top level" }).detail)
    .toBe(t().orphanEvFileCount(12));
  expect(evidenceText({ ...sample, code: "orphan_mtime", detail: "31 day(s) ago" }).detail)
    .toBe(t().orphanEvDays(31));
  expect(evidenceText({ ...sample, code: "orphan_root", detail: "Program Files" }).detail).toBe("Program Files");
  expect(evidenceText({ ...sample, code: "orphan_recent", detail: "written 2 day(s) ago" }))
    .toEqual({ label: t().orphanEvRecentLabel, detail: t().orphanEvRecent(2) });
  expect(t().scanEvidenceLabel("service_bound", "")).not.toBe("");
  expect(evidenceText({ ...sample, code: "service_bound", label: "Registered service uses this path" }).label)
    .toBe(t().scanEvidenceLabel("service_bound", ""));
  expect(evidenceText({ ...sample, code: "other" })).toEqual({ ...sample, code: "other" });
});
