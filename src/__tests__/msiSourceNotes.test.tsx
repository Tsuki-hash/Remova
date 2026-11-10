// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { MsiPanel } from "../components/AuditPanels";
import { api } from "../lib/api";
import { currentLang, setLang, t, type Lang } from "../i18n";
import type { MsiConsistencyReport } from "../types";
import fixtures from "./fixtures/msi-source-notes.json";

vi.mock("../lib/api", () => ({ api: { msiConsistency: vi.fn() } }));

const originalLang = currentLang();
afterEach(() => {
  cleanup();
  vi.mocked(api.msiConsistency).mockReset();
  setLang(originalLang);
});

function report(name: string): MsiConsistencyReport {
  const fixture = fixtures.find(row => row.name === name)!;
  return {
    notes: fixture.notes,
    completeness: {
      status: fixture.status as "complete" | "partial",
      roots_scanned: fixture.roots_scanned,
      failed_roots: fixture.failed_roots,
    },
    products: [{
      product_code: "{593674B6-8B0D-4E23-8413-6B0E0DA19552}",
      display_name: "Fixture product",
      install_location: "",
      location_exists: null,
      has_uninstall_record: name !== "inventory_unknown",
      has_installer_key: name !== "installer_failed",
      in_msi_db: name !== "database_failed",
      class: fixture.class,
    }],
  };
}

for (const lang of ["zh", "en"] satisfies Lang[]) {
  describe(`MSI source notes (${lang})`, () => {
    it.each(fixtures.map(row => row.name))("renders each actual source in %s", async name => {
      setLang(lang);
      const L = t();
      vi.mocked(api.msiConsistency).mockResolvedValue(report(name));
      render(<MsiPanel onClose={vi.fn()} />);
      await screen.findByText("Fixture product");
      const expected: string[] = {
        complete: [L.msiUninstallOk, L.msiInstallerKeysOk, L.msiDbOk],
        inventory_unknown: [L.msiUninstallUnknown, L.msiInstallerKeysOk, L.msiDbOk],
        installer_failed: [L.msiUninstallOk, L.msiInstallerKeysUnavailable, L.msiDbOk],
        database_failed: [L.msiUninstallOk, L.msiInstallerKeysOk, L.msiDbUnavailable],
      }[name]!;
      for (const text of new Set(expected)) expect(screen.getAllByText(text).length).toBeGreaterThan(0);
      for (const text of [
        L.msiUninstallOk, L.msiUninstallUnknown, L.msiUninstallUnavailable,
        L.msiInstallerKeysOk, L.msiInstallerKeysUnavailable, L.msiDbOk,
        L.msiDbUnavailable, L.msiSourceUnknown,
      ]) {
        if (!expected.includes(text)) expect(screen.queryByText(text)).toBeNull();
      }
      if (name === "complete") {
        expect(screen.queryByRole("note")).toBeNull();
        expect(screen.getByText(L.msiClassConsistent)).toBeTruthy();
      } else {
        expect(screen.getByRole("note").textContent).toContain(L.scanCompletenessPartial);
        expect(screen.getByText(L.msiClassInconclusive)).toBeTruthy();
        expect(screen.queryByText("inconclusive")).toBeNull();
      }
      // Database success describes only its own source, never three-view completion.
      expect(L.msiDbOk).not.toMatch(/三方对照完整|all three views answered/);
    });

    it("does not blame the database for inventory errors or unknown notes", async () => {
      setLang(lang);
      const L = t();
      const payload = report("inventory_unknown");
      payload.notes = [
        "uninstall_unavailable:fixture inventory denied",
        "installer_keys_unreadable:legacy root",
        "future_source_ok",
        "msi_db_ok",
      ];
      vi.mocked(api.msiConsistency).mockResolvedValue(payload);
      render(<MsiPanel onClose={vi.fn()} />);
      await screen.findByText(L.msiUninstallUnavailable);
      expect(screen.getByText(L.msiInstallerKeysUnavailable)).toBeTruthy();
      expect(screen.getByText(L.msiSourceUnknown)).toBeTruthy();
      expect(screen.getByText(L.msiDbOk)).toBeTruthy();
      expect(screen.queryByText(L.msiDbUnavailable)).toBeNull();
    });
  });
}
