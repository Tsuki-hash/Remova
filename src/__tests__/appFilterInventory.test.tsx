// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, renderHook } from "@testing-library/react";
import { useAppFilter } from "../hooks/useAppFilter";
import type { InstalledApp } from "../types";

afterEach(cleanup);
it("maps Copilot identities to current inventory and drops removed products", () => {
  const old: InstalledApp = { name: "Product", publisher: "Vendor", source: "HKCU", registry_key: "identity",
    version: "old", install_location: "/old", uninstall_string: "old-command", quiet_uninstall_string: "",
    estimated_size_kb: 1, install_date: "", display_icon: "" };
  const current = { ...old, version: "new", install_location: "/new", uninstall_string: "new-command" };
  const options: Parameters<typeof useAppFilter>[0] = { apps: [old], copilotList: [old], deferredQ: "",
    category: "all", sortCol: null, sortDesc: false, sizeOf: a => a.estimated_size_kb,
    ignorePub: [], ignoreName: [], setSortCol: vi.fn(), setSortDesc: vi.fn() };
  const { result, rerender } = renderHook(props => useAppFilter(props), { initialProps: options });
  rerender({ ...options, apps: [current] });
  expect(result.current.filtered).toEqual([current]);
  expect(result.current.filtered[0]).toBe(current);
  const other = { ...old, name: "Other", registry_key: "other" };
  rerender({ ...options, apps: [other, current], copilotList: [old, other] });
  expect(result.current.filtered).toEqual([current, other]);
  rerender({ ...options, apps: [] });
  expect(result.current.filtered).toEqual([]);
  rerender({ ...options, apps: [current], copilotList: [] });
  expect(result.current.filtered).toEqual([]);
  rerender({ ...options, apps: [current], copilotList: null });
  expect(result.current.filtered).toEqual([current]);
});
