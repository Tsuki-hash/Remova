// @vitest-environment jsdom
import { useState } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { ScanLeftoversView } from "../components/ScanLeftoversView";
import { setLang, t } from "../i18n";
import type { CleanupItem, InstalledApp } from "../types";
vi.mock("@tanstack/react-virtual", () => ({ useVirtualizer: ({ count }: { count: number }) => ({
  getVirtualItems: () => Array.from({ length: count }, (_, index) => ({ index, start: index * 72 })),
  getTotalSize: () => count * 72, measureElement: vi.fn(),
}) }));
afterEach(() => { cleanup(); setLang("zh"); });
const item = (path: string, kind: "file" | "registry", bucket: string): CleanupItem => ({ path, kind, bucket,
  score: 90, confidence: "confirmed", risk: "low", reason: "", evidence: [] });
const items = [item("C:\\App\\one", "file", "programFiles"), item("HKCU\\Software\\sample", "registry", "registry")];
const app = { name: "sample", install_location: "C:\\App" } as InstalledApp;

it.each(["zh", "en"] as const)("keeps hidden selection and combines Selected only with the type filter in %s", lang => {
  setLang(lang);
  const toggle = vi.fn();
  render(<ScanLeftoversView scan={{ app_name: "sample", items }} scanning={false} selectedPaths={new Set([items[0]!.path])}
    evidence={null} aiNotes={{}} orphanLabel="orphans" onTogglePath={toggle} onEvidence={() => {}}
    kindFilter="registry" filterApp={app} onClearKindFilter={() => {}} />);
  expect(screen.getByText(t().scanSelectionScope(1, 1, 1))).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: t().onlySelected }));
  expect(screen.getByText(t().selectedFilterEmpty)).toBeTruthy();
  expect(screen.getByText(t().scanSelectionScope(0, 1, 1))).toBeTruthy();
  expect(toggle).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: t().onlySelected }));
  expect(screen.getByRole("checkbox", { name: items[1]!.path })).toBeTruthy();
});

it("keeps the exit toggle after unchecking the final visible selection", () => {
  function Harness() {
    const [selected, setSelected] = useState(new Set([items[0]!.path]));
    return <ScanLeftoversView scan={{ app_name: "sample", items }} scanning={false} selectedPaths={selected}
      evidence={null} aiNotes={{}} orphanLabel="orphans" onEvidence={() => {}}
      onTogglePath={path => setSelected(previous => { const next = new Set(previous); next.delete(path); return next; })} />;
  }
  render(<Harness />);
  fireEvent.click(screen.getByRole("button", { name: t().onlySelected }));
  fireEvent.click(screen.getByRole("checkbox", { name: items[0]!.path }));
  expect(screen.getByText(t().selectedFilterEmpty)).toBeTruthy();
  expect(screen.getByRole("button", { name: t().onlySelected }).getAttribute("aria-pressed")).toBe("true");
  fireEvent.click(screen.getByRole("button", { name: t().onlySelected }));
  expect(screen.getAllByRole("checkbox")).toHaveLength(2);
});
