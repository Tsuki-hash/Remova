// @vitest-environment jsdom
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { SoftwareListTable } from "../components/SoftwareListTable";
import type { InstalledApp } from "../types";

function app(n: number): InstalledApp {
  return {
    name: `Demo App ${n}`,
    version: "1.0",
    publisher: "Acme",
    install_location: `C:\\Program Files\\Demo${n}`,
    uninstall_string: `"C:\\Program Files\\Demo${n}\\uninst.exe"`,
    quiet_uninstall_string: "",
    source: "HKLM64",
    registry_key: `HKLM64\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Demo${n}`,
    estimated_size_kb: 2048,
    install_date: "20260101",
    display_icon: "",
  };
}

const noop = () => {};

function props(over: Partial<Record<string, unknown>> = {}) {
  return {
    filtered: [app(1), app(2), app(3)],
    loading: false,
    q: "",
    category: "all" as const,
    sortCol: "name" as const,
    sortDesc: false,
    selected: null,
    multi: new Set<string>(),
    uninstallingKey: null,
    appKey: (a: InstalledApp) => a.name,
    sizeText: () => "2 MB",
    sizeOf: (a: InstalledApp) => a.estimated_size_kb,
    sortBy: noop,
    selectApp: noop,
    startUninstall: noop,
    analyze: noop,
    forceClean: noop,
    doIgnoreApp: noop,
    doIgnorePublisher: noop,
    toggleMulti: noop,
    setSelected: noop,
    ...over,
  };
}

beforeEach(() => cleanup());

// jsdom reports 0x0 for every element, which makes the virtualizer compute an empty viewport and
// render no rows. Give elements a real box before mount so the virtualization path is exercised.
const VIEWPORT_H = 600;
const VIEWPORT_W = 1100;

beforeAll(() => {
  for (const [prop, value] of [
    ["clientHeight", VIEWPORT_H],
    ["offsetHeight", VIEWPORT_H],
    ["clientWidth", VIEWPORT_W],
    ["offsetWidth", VIEWPORT_W],
  ] as const) {
    Object.defineProperty(HTMLElement.prototype, prop, {
      configurable: true,
      value,
    });
  }
  Object.defineProperty(Element.prototype, "getBoundingClientRect", {
    configurable: true,
    value: function (this: Element) {
      return {
        width: VIEWPORT_W,
        height: this === document.body ? VIEWPORT_H : 60,
        top: 0,
        left: 0,
        bottom: VIEWPORT_H,
        right: VIEWPORT_W,
        x: 0,
        y: 0,
        toJSON: () => ({}),
      };
    },
  });
});

describe("SoftwareListTable", () => {
  it("renders virtual rows tagged with their index so they can be measured", () => {
    const { container } = render(<SoftwareListTable {...props()} />);
    const rows = Array.from(
      container.querySelectorAll<HTMLTableRowElement>("tbody tr[data-index]"),
    );
    expect(rows.length).toBeGreaterThan(0);
    // data-index is what rowVirtualizer.measureElement keys measurements by.
    expect(rows[0]?.dataset.index).toBeDefined();
    expect(screen.getAllByText(/Demo App/).length).toBeGreaterThan(0);
  });

  it("row click selects the app and checkbox toggles multi-select", () => {
    const selectApp = vi.fn();
    const toggleMulti = vi.fn();
    const { container } = render(
      <SoftwareListTable {...props({ selectApp, toggleMulti })} />,
    );
    const row = container.querySelector<HTMLTableRowElement>("tbody tr[data-index]");
    if (!row) throw new Error("no virtual row rendered");
    fireEvent.click(row);
    expect(selectApp).toHaveBeenCalledTimes(1);
    expect(selectApp).toHaveBeenCalledWith(expect.objectContaining({ name: "Demo App 1" }));

    const box = row.querySelector('input[type="checkbox"]');
    if (!box) throw new Error("no row checkbox");
    fireEvent.click(box);
    expect(toggleMulti).toHaveBeenCalledWith("Demo App 1");
  });

  it("shows the empty state without throwing when nothing matches", () => {
    render(<SoftwareListTable {...props({ filtered: [] })} />);
    expect(screen.queryAllByText(/Demo App/)).toHaveLength(0);
  });

  it("renders skeletons while loading with no data yet", () => {
    const { container } = render(
      <SoftwareListTable {...props({ filtered: [], loading: true })} />,
    );
    expect(container.querySelectorAll(".remova-skeleton").length).toBeGreaterThan(0);
  });
});
