import { afterEach, describe, expect, it, vi } from "vitest";
import { acquireUpdateSlot, trackNativeCall, UPDATE_BUSY } from "../lib/nativeActivity";

afterEach(() => {
  // release the slot even if an assertion fired
  vi.resetModules();
});

describe("update slot vs read-only commands", () => {
  it("lets read-only commands through while mutating ones see update:busy", async () => {
    const release = acquireUpdateSlot();
    expect(release).not.toBeNull();
    await expect(trackNativeCall(async () => "apps", "list_installed_apps")).resolves.toBe("apps");
    await expect(trackNativeCall(async () => 1, "delete_cleanup_history")).rejects.toThrow(UPDATE_BUSY);
    await expect(trackNativeCall(async () => 1)).rejects.toThrow(UPDATE_BUSY);
    release?.();
    await expect(trackNativeCall(async () => 1, "delete_cleanup_history")).resolves.toBe(1);
  });

  it("refuses a second slot while one is held or calls are active", async () => {
    const release = acquireUpdateSlot();
    expect(acquireUpdateSlot()).toBeNull();
    release?.();
    expect(acquireUpdateSlot()).not.toBeNull();
    release?.();
  });
});
