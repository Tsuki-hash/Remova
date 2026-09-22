import { beforeEach, describe, expect, it } from "vitest";
import { dismissToast, getToasts, toast } from "../lib/toast";

describe("toast channel", () => {
  beforeEach(() => {
    toast.clear();
  });

  it("replaces previous toast in the same channel", () => {
    toast.info("扫描中", { channel: "orphan-scan", sticky: true });
    const first = getToasts();
    expect(first).toHaveLength(1);
    expect(first[0].message).toBe("扫描中");
    toast.success("扫描完成", { channel: "orphan-scan" });
    const second = getToasts();
    expect(second).toHaveLength(1);
    expect(second[0].message).toBe("扫描完成");
    expect(second[0].channel).toBe("orphan-scan");
  });

  it("keeps unrelated channels independent", () => {
    toast.info("A", { channel: "a", sticky: true });
    toast.info("B", { channel: "b", sticky: true });
    expect(getToasts()).toHaveLength(2);
  });

  it("dismiss works", () => {
    const id = toast.info("x", { channel: "c" });
    expect(getToasts()).toHaveLength(1);
    dismissToast(id);
    expect(getToasts()).toHaveLength(0);
  });
});
