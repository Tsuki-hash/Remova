// @vitest-environment jsdom
import { afterEach, expect, it } from "vitest";
import { setLang, t } from "../i18n";
import { evidenceText } from "../lib/evidenceText";
afterEach(() => setLang("zh"));
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
