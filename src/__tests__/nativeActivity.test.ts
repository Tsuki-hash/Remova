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
    for (const command of ["list_installed_apps", "list_local_drives", "list_top_dir_sizes", "list_dir_children", "rank_idle_apps", "app_icon_data", "estimate_dir_size_kb", "list_startup_items", "list_services", "list_scheduled_tasks", "verify_cleanup_leftovers"]) {
      await expect(trackNativeCall(async () => "apps", command)).resolves.toBe("apps");
    }
    for (const command of ["scan_orphan_leftovers", "take_pending_analyze"]) {
      await expect(trackNativeCall(async () => 1, command)).rejects.toThrow(UPDATE_BUSY);
    }
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
