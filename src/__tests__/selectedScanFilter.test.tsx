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

it.each(["zh", "en"] as const)("selects the whole filtered result and preserves hidden selections in %s", lang => {
  setLang(lang);
  const extra = { ...item("HKCU\\Software\\review", "registry", "registry"), risk: "medium" as const };
  function Harness() {
    const [selected, setSelected] = useState(new Set(items.map(item => item.path)));
    return <ScanLeftoversView scan={{ app_name: "sample", items: [...items, extra] }} scanning={false}
      selectedPaths={selected} evidence={null} aiNotes={{}} orphanLabel="orphans" onEvidence={() => {}}
      onTogglePath={() => {}} kindFilter="registry" filterApp={app}
      onSelectVisible={(paths, checked) => setSelected(previous => {
        const next = new Set(previous);
        for (const path of paths) { if (checked) next.add(path); else next.delete(path); }
        return next;
      })} />;
  }
  render(<Harness />);
  const all = screen.getByRole<HTMLInputElement>("checkbox", { name: t().selectVisible });
  expect(all.indeterminate).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: t().selectVisible }));
  expect(all.checked).toBe(true);
  expect(all.indeterminate).toBe(false);
  expect(screen.getByRole<HTMLInputElement>("checkbox", { name: extra.path }).checked).toBe(true);
  expect(screen.getByText(t().scanSelectionScope(2, 3, 1))).toBeTruthy();
  fireEvent.click(all);
  expect(all.checked).toBe(false);
  expect(screen.getByText(t().scanSelectionScope(2, 1, 1))).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: t().onlySelected }));
  expect(all.disabled).toBe(true);
  expect(screen.getByRole<HTMLButtonElement>("button", { name: t().selectVisible }).disabled).toBe(true);
});

it("toggles a row once, keeps evidence independent and blocks selection during scanning", () => {
  const toggle = vi.fn();
  const evidence = vi.fn();
  function Harness({ scanning = false }: { scanning?: boolean }) {
    const [selected, setSelected] = useState(new Set<string>());
    return <ScanLeftoversView scan={{ app_name: "sample", items }} scanning={scanning}
      selectedPaths={selected} evidence={null} aiNotes={{}} orphanLabel="orphans" onEvidence={evidence}
      onTogglePath={path => { toggle(path); setSelected(previous => {
        const next = new Set(previous); if (next.has(path)) next.delete(path); else next.add(path); return next;
      }); }} />;
  }
  const view = render(<Harness />);
  const box = screen.getByRole<HTMLInputElement>("checkbox", { name: items[0]!.path });
  fireEvent.click(screen.getByText(items[0]!.path));
  expect(box.checked).toBe(true);
  fireEvent.click(box);
  expect(box.checked).toBe(false);
  expect(toggle).toHaveBeenCalledTimes(2);
  fireEvent.click(screen.getByRole("button", { name: `${t().orphanEvidenceTitle}: ${items[0]!.path}` }));
  expect(evidence).toHaveBeenCalledOnce();
  expect(toggle).toHaveBeenCalledTimes(2);
  view.rerender(<Harness scanning />);
  fireEvent.click(screen.getByText(items[0]!.path));
  fireEvent.click(box);
  expect(box.disabled).toBe(true);
  expect(toggle).toHaveBeenCalledTimes(2);
});

it("renders evidence in the active language after switching a memoized row", () => {
  const sample = { ...items[1]!, evidence: [{ code: "shell_class", label: "Classes key name matches product", weight: 35, detail: "xiaomi-mimo" }] };
  function Harness() {
    const [evidence, setEvidence] = useState<string | null>(null);
    return <ScanLeftoversView scan={{ app_name: "sample", items: [sample] }} scanning={false}
      selectedPaths={new Set()} evidence={evidence} aiNotes={{}} orphanLabel="orphans"
      onTogglePath={() => {}} onEvidence={setEvidence} />;
  }
  setLang("zh");
  const view = render(<Harness />);
  fireEvent.click(screen.getByRole("button", { name: `${t().orphanEvidenceTitle}: ${sample.path}` }));
  expect(screen.getByText("Classes 注册项名称匹配软件 (35) — xiaomi-mimo")).toBeTruthy();
  setLang("en");
  view.rerender(<Harness />);
  fireEvent.click(screen.getByRole("button", { name: `${t().orphanEvidenceTitle}: ${sample.path}` }));
  expect(screen.getByText("Classes key name matches product (35) — xiaomi-mimo")).toBeTruthy();
});

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
