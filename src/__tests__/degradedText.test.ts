/** stable degraded codes must map to localized text; anything else shows raw. */
import { describe, expect, it } from "vitest";
import { degradedText } from "../lib/degradedText";
import { t } from "../i18n";

describe("degradedText", () => {
  const L = t();

  it("maps the unreadable-root code with the path preserved", () => {
    expect(degradedText("unreadable_root:C:\\Users\\u\\npm-cache")).toBe(
      L.degradedUnreadableRoot("C:\\Users\\u\\npm-cache"),
    );
  });

  it("truncates a code without a path to a question mark placeholder", () => {
    expect(degradedText("unreadable_root:")).toBe(L.degradedUnreadableRoot("?"));
  });

  it("maps each named code", () => {
    expect(degradedText("downloads_root_missing")).toBe(L.degradedDownloadsRootMissing);
    expect(degradedText("userprofile_not_set")).toBe(L.degradedProfileNotSet);
    expect(degradedText("localappdata_not_set")).toBe(L.degradedLocalAppDataNotSet);
    expect(degradedText("pe_budget_exhausted:37")).toBe(L.degradedPeBudgetExhausted(37));
    expect(degradedText("empty_shells_skipped:3")).toBe(L.degradedEmptyShellsSkipped(3));
    expect(degradedText("home_dot_truncated")).toBe(L.degradedHomeDotTruncated);
    expect(degradedText("muicache_truncated")).toBe(L.degradedMuicacheTruncated);
    expect(degradedText("temp_partial")).toBe(L.degradedTempPartial);
  });

  // The backend sends `<code>:<count>`; a suffix that is not a number must
  // fall back to 0 instead of NaN in either counted code.
  it("falls back to 0 for a non-numeric count suffix", () => {
    expect(degradedText("pe_budget_exhausted:later")).toBe(L.degradedPeBudgetExhausted(0));
    expect(degradedText("empty_shells_skipped:x")).toBe(L.degradedEmptyShellsSkipped(0));
  });

  it("passes unrecognized codes through unchanged (older/newer backend tolerance)", () => {
    expect(degradedText("some-future-code")).toBe("some-future-code");
    expect(degradedText("C:\\some\\raw\\path-from-an-older-backend")).toBe(
      "C:\\some\\raw\\path-from-an-older-backend",
    );
  });
});
