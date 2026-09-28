import { afterEach, describe, expect, it } from "vitest";
import {
  getConfirm,
  getConfirmChecked,
  requestConfirm,
  requestConfirmEx,
  settleConfirm,
  setConfirmChecked,
  subscribeConfirm,
} from "../lib/confirm";

describe("confirm store", () => {
  afterEach(() => {
    if (getConfirm()) settleConfirm(false);
    setConfirmChecked(false);
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

  it("requestConfirmEx defaults checkbox to unchecked", async () => {
    const p = requestConfirmEx({
      title: "backup",
      checkbox: { label: "Create safety backup" },
    });
    expect(getConfirmChecked()).toBe(false);
    settleConfirm(true);
    await expect(p).resolves.toEqual({ ok: true, checked: false });
  });

  it("requestConfirmEx returns checked state when user opts in", async () => {
    const p = requestConfirmEx({
      title: "backup2",
      checkbox: { label: "Create safety backup", defaultChecked: true },
    });
    expect(getConfirmChecked()).toBe(true);
    setConfirmChecked(false);
    setConfirmChecked(true);
    settleConfirm(true);
    await expect(p).resolves.toEqual({ ok: true, checked: true });
  });

  it("cancel ignores checkbox", async () => {
    const p = requestConfirmEx({
      title: "backup3",
      checkbox: { label: "Create safety backup", defaultChecked: true },
    });
    settleConfirm(false);
    await expect(p).resolves.toEqual({ ok: false, checked: false });
  });
});
