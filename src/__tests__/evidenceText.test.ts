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
  expect(evidenceText({ ...sample, code: "other" })).toEqual({ ...sample, code: "other" });
});
