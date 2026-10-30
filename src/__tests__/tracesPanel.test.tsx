// @vitest-environment jsdom
// The traces panel used to be a dead-end list: no plain-language purpose and
// no row actions. These tests pin the new-user guidance and the row actions.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { TracesPanel } from "../components/TracesPanel";
import { t } from "../i18n";
import { toast } from "../lib/toast";

const native = vi.hoisted(() => ({
  consentOrphans: vi.fn(),
  portableCandidates: vi.fn(),
  openPath: vi.fn(),
}));

vi.mock("../lib/api", () => ({ api: native }));

beforeEach(() => {
  vi.clearAllMocks();
  native.consentOrphans.mockResolvedValue({ consent_orphans: [], omitted: 0, completeness: { status: "complete", roots_scanned: 1, failed_roots: [] } });
  native.portableCandidates.mockResolvedValue({ candidates: [], omitted: 0 });
  native.openPath.mockResolvedValue(undefined);
});
afterEach(() => {
  cleanup();
  toast.clear();
});

describe("TracesPanel", () => {
  it("explains the read-only checkup and shows friendly empty states", async () => {
    render(<TracesPanel onClose={() => {}} />);
    const L = t();
    expect(screen.getByText(L.tracesIntro)).toBeTruthy();
    expect(await screen.findByText(L.tracesConsentEmpty)).toBeTruthy();
    expect(screen.getByText(L.tracesPortableEmpty)).toBeTruthy();
    expect(screen.queryByRole("button", { name: L.tracesConsentSettings })).toBeNull();
    expect(screen.getByText(L.tracesTechSummary)).toBeTruthy();
    expect(document.querySelector("details")?.open).toBe(false);
  });

  it("offers row actions: open location for portable apps, copy path for both", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    native.consentOrphans.mockResolvedValue({
      consent_orphans: [
        { capability: "microphone", key: "HKLM\\...\\NonPackaged", exe_path: "C:\\Gone\\gone.exe" },
      ],
      omitted: 0,
      completeness: { status: "complete", roots_scanned: 1, failed_roots: [] },
    });
    native.portableCandidates.mockResolvedValue({
      candidates: [{ exe_path: "D:\\Portable\\tool.exe", rule_name: "Tool rule" }],
      omitted: 2,
    });

    render(<TracesPanel onClose={() => {}} />);
    const L = t();
    expect(await screen.findByText("C:\\Gone\\gone.exe")).toBeTruthy();
    expect(screen.getByText(`${L.tracesCapabilityPrefix}microphone`)).toBeTruthy();
    expect(screen.getByText(`${L.tracesRulePrefix}Tool rule`)).toBeTruthy();
    expect(screen.getByText(L.scanOmitted(2))).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: `${L.openLocation}: D:\\Portable\\tool.exe` }));
    await waitFor(() => expect(native.openPath).toHaveBeenCalledWith("D:\\Portable\\tool.exe"));

    fireEvent.click(screen.getByRole("button", { name: L.tracesConsentSettings }));
    await waitFor(() => expect(native.openPath).toHaveBeenCalledWith("ms-settings:privacy"));

    fireEvent.click(screen.getByRole("button", { name: `${L.copyPath}: C:\\Gone\\gone.exe` }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("C:\\Gone\\gone.exe"));
  });

  it("rescans on demand and closes through the header button", async () => {
    const onClose = vi.fn();
    render(<TracesPanel onClose={onClose} />);
    const L = t();
    await screen.findByText(L.tracesConsentEmpty);
    expect(native.consentOrphans).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: L.manageReload }));
    await waitFor(() => expect(native.consentOrphans).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(native.portableCandidates).toHaveBeenCalledTimes(2));

    fireEvent.click(screen.getByRole("button", { name: L.panelClose }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("surfaces a failed scan as an error instead of an empty result", async () => {
    native.consentOrphans.mockRejectedValue("scan denied");
    render(<TracesPanel onClose={() => {}} />);
    expect((await screen.findByRole("alert")).textContent).toContain("scan denied");
    expect(screen.queryByText(t().tracesConsentEmpty)).toBeNull();
  });
});
