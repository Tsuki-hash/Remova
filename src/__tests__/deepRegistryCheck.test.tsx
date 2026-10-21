// @vitest-environment jsdom
// M5-4: the deep registry check is explicit and read-only — it must never
// render a delete control, and degraded results must say so.
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { DeepRegistryCheck } from "../components/DeepRegistryCheck";
import { setLang, t } from "../i18n";

const deepRegistryCheck = vi.fn();
vi.mock("../lib/api", () => ({
  api: { deepRegistryCheck: (...a: unknown[]) => deepRegistryCheck(...a) },
}));

afterEach(cleanup);
beforeEach(() => {
  vi.clearAllMocks();
  setLang("zh");
});

it("reports matching keys read-only and never offers a delete control", async () => {
  deepRegistryCheck.mockResolvedValue({
    hits: [
      { key: "HKLM64\\SOFTWARE\\Acme\\AcmeTool", name: "AcmeTool", detail: "nested" },
      { key: "HKLM64\\SOFTWARE\\Vendor\\Acme", name: "Acme", detail: "nested_weak" },
    ],
    scanned: 42,
    truncated: false,
    timed_out: false,
  });
  render(<DeepRegistryCheck appName="Acme Tool" />);
  fireEvent.click(screen.getByRole("button", { name: t().deepRegistryCheck }));
  await screen.findByText("HKLM64\\SOFTWARE\\Acme\\AcmeTool");
  expect(deepRegistryCheck).toHaveBeenCalledWith("Acme Tool");
  expect(screen.getByText(t().deepRegistryReadOnly)).toBeTruthy();
  expect(screen.getByText(t().deepRegistryHits(2))).toBeTruthy();
  expect(screen.getByText(t().deepRegistryNested, { exact: false })).toBeTruthy();
  expect(screen.getByText(t().deepRegistryNestedWeak, { exact: false })).toBeTruthy();
  expect(screen.queryByRole("button", { name: /delete|删除/i })).toBeNull();
});

it("surfaces bounded-scan warnings and errors", async () => {
  deepRegistryCheck.mockResolvedValueOnce({
    hits: [],
    scanned: 4000,
    truncated: true,
    timed_out: true,
  });
  const view = render(<DeepRegistryCheck appName="Acme Tool" />);
  fireEvent.click(screen.getByRole("button", { name: t().deepRegistryCheck }));
  await screen.findByText(t().deepRegistryEmpty);
  expect(screen.getByText(t().deepRegistryTruncated)).toBeTruthy();
  expect(screen.getByText(t().deepRegistryTimedOut)).toBeTruthy();
  view.unmount();

  deepRegistryCheck.mockRejectedValueOnce(new Error("registry denied"));
  render(<DeepRegistryCheck appName="Acme Tool" />);
  fireEvent.click(screen.getByRole("button", { name: t().deepRegistryCheck }));
  await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("registry denied"));
  cleanup();

  deepRegistryCheck.mockRejectedValueOnce(new Error("deep_registry_too_generic"));
  render(<DeepRegistryCheck appName="QQ" />);
  fireEvent.click(screen.getByRole("button", { name: t().deepRegistryCheck }));
  await waitFor(() =>
    expect(screen.getByRole("alert").textContent).toContain(t().deepRegistryTooGeneric),
  );
});
