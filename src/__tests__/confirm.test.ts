import { afterEach, describe, expect, it } from "vitest";
import {
  getConfirm,
  requestConfirm,
  settleConfirm,
  subscribeConfirm,
} from "../lib/confirm";

describe("confirm store", () => {
  afterEach(() => {
    if (getConfirm()) settleConfirm(false);
  });

  it("requestConfirm publishes options and resolves true on settle", async () => {
    const p = requestConfirm({ title: "T", message: "M", danger: true });
    expect(getConfirm()?.title).toBe("T");
    expect(getConfirm()?.danger).toBe(true);
    settleConfirm(true);
    await expect(p).resolves.toBe(true);
    expect(getConfirm()).toBeNull();
  });

  it("resolve false on cancel", async () => {
    const p = requestConfirm({ title: "T2" });
    settleConfirm(false);
    await expect(p).resolves.toBe(false);
  });

  it("new request cancels the previous pending dialog", async () => {
    const a = requestConfirm({ title: "A" });
    const b = requestConfirm({ title: "B" });
    await expect(a).resolves.toBe(false);
    expect(getConfirm()?.title).toBe("B");
    settleConfirm(true);
    await expect(b).resolves.toBe(true);
  });

  it("subscribeConfirm notifies listeners", () => {
    let n = 0;
    const unsub = subscribeConfirm(() => {
      n += 1;
    });
    const p = requestConfirm({ title: "sub" });
    expect(n).toBeGreaterThanOrEqual(1);
    settleConfirm(false);
    void p;
    unsub();
  });
});
