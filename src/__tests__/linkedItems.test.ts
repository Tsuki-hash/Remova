import { describe, expect, it } from "vitest";
import type { CleanupItem, InstalledApp } from "../types";
import {
  buildLinkedBuckets,
  classifyItem,
  filterItemsByBucket,
  linkedBucketLabelKey,
} from "../lib/linkedItems";

const app: InstalledApp = {
  name: "DemoApp",
  version: "1.0",
  publisher: "Demo",
  install_location: "C:\\Program Files\\DemoApp",
  uninstall_string: "x",
  quiet_uninstall_string: "",
  source: "HKLM64",
  registry_key: "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Demo",
  estimated_size_kb: 100,
  install_date: "20260101",
  display_icon: "",
};

function item(partial: Partial<CleanupItem> & { path: string; kind: CleanupItem["kind"] }): CleanupItem {
  return {
    score: 90,
    confidence: "confirmed",
    risk: "low",
    reason: "test",
    evidence: [],
    ...partial,
  };
}

describe("classifyItem", () => {
  it("maps install-location dir to programFiles", () => {
    expect(
      classifyItem(item({ path: "C:\\Program Files\\DemoApp\\bin", kind: "dir" }), app),
    ).toBe("programFiles");
  });

  it("maps AppData path to configFiles", () => {
    expect(
      classifyItem(
        item({ path: "C:\\Users\\u\\AppData\\Local\\DemoApp", kind: "dir" }),
        app,
      ),
    ).toBe("configFiles");
  });

  it("maps registry uninstall key to registry", () => {
    expect(
      classifyItem(
        item({
          path: "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Demo",
          kind: "registry",
        }),
        app,
      ),
    ).toBe("registry");
  });

  it("maps Run key to startup", () => {
    expect(
      classifyItem(
        item({
          path: "HKCU\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run\\Demo",
          kind: "registry",
        }),
        app,
      ),
    ).toBe("startup");
  });

  it("maps .lnk under install to shortcuts", () => {
    expect(
      classifyItem(
        item({ path: "C:\\ProgramData\\Microsoft\\Windows\\Start Menu\\Demo.lnk", kind: "file" }),
        app,
      ),
    ).toBe("shortcuts");
  });

  it("maps Startup folder lnk to startup", () => {
    expect(
      classifyItem(
        item({
          path: "C:\\Users\\u\\AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Startup\\Demo.lnk",
          kind: "file",
        }),
        app,
      ),
    ).toBe("startup");
  });

  it("prefers backend bucket when present", () => {
    expect(
      classifyItem(
        item({
          path: "HKLM\\SOFTWARE\\Demo",
          kind: "registry",
          bucket: "programFiles",
        }),
        app,
      ),
    ).toBe("programFiles");
  });

  it("maps PATH kind to other", () => {
    expect(classifyItem(item({ path: "C:\\Tools\\Demo", kind: "path" }), app)).toBe("other");
  });
});

describe("buildLinkedBuckets", () => {
  it("orders buckets and sums sizes only for file/dir", () => {
    const items = [
      item({ path: "C:\\Program Files\\DemoApp\\a.bin", kind: "file", size_kb: 100 }),
      item({ path: "C:\\Program Files\\DemoApp\\b.bin", kind: "file", size_kb: 50 }),
      item({ path: "HKLM\\SOFTWARE\\Demo", kind: "registry", size_kb: null }),
      item({
        path: "HKCU\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run\\Demo",
        kind: "registry",
      }),
    ];
    const buckets = buildLinkedBuckets(items, app);
    expect(buckets.map((b) => b.id)).toEqual(["programFiles", "registry", "startup"]);
    expect(buckets[0]).toEqual({ id: "programFiles", count: 2, sizeKb: 150 });
    expect(buckets[1].sizeKb).toBeNull();
  });

  it("omits empty buckets", () => {
    const buckets = buildLinkedBuckets(
      [item({ path: "HKLM\\SOFTWARE\\Demo", kind: "registry" })],
      app,
    );
    expect(buckets).toHaveLength(1);
    expect(buckets[0].id).toBe("registry");
  });

  it("sizeKb null when file sizes missing", () => {
    const buckets = buildLinkedBuckets(
      [item({ path: "C:\\Program Files\\DemoApp\\x.bin", kind: "file", size_kb: null })],
      app,
    );
    expect(buckets[0].sizeKb).toBeNull();
    expect(buckets[0].count).toBe(1);
  });
});

describe("filterItemsByBucket", () => {
  it("filters by bucket and passes all when null", () => {
    const items = [
      item({ path: "C:\\Program Files\\DemoApp\\a.bin", kind: "file" }),
      item({ path: "HKLM\\SOFTWARE\\Demo", kind: "registry" }),
    ];
    expect(filterItemsByBucket(items, app, "registry")).toHaveLength(1);
    expect(filterItemsByBucket(items, app, null)).toHaveLength(2);
  });
});

describe("linkedBucketLabelKey", () => {
  it("returns i18n keys", () => {
    expect(linkedBucketLabelKey("programFiles")).toBe("linkedProgramFiles");
    expect(linkedBucketLabelKey("other")).toBe("linkedOther");
  });
});
