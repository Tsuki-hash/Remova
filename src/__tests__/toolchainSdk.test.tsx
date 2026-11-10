// @vitest-environment jsdom
// The backend runs SDK probes; without a consumer the result never reached the
// user and structural typing hid the missing field.
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { ToolchainPanel } from "../components/ToolchainPanel";
import { setLang, t } from "../i18n";

const auditToolchains = vi.fn();
vi.mock("../lib/api", () => ({
  api: {
    auditToolchains: (...a: unknown[]) => auditToolchains(...a),
    toolchainConsumers: vi.fn(async () => ({ hits: [], omitted: 0, completeness: { status: "complete", roots_scanned: 0, failed_roots: [] } })),
  },
}));
afterEach(cleanup);
beforeEach(() => {
  vi.clearAllMocks();
  setLang("zh");
});

it("shows SDK probe results returned by the backend", async () => {
  auditToolchains.mockResolvedValue({
    vswhere_available: true,
    instances: [],
    sdks: [
      { tool: "dotnet", present: true, detail: "8.0.100" },
      { tool: "java", present: false, detail: "" },
    ],
    package_cache_kb: 0,
    package_cache_capped: false,
    notes: [],
    completeness: { status: "complete", roots_scanned: 1, failed_roots: [] },
  });
  render(<ToolchainPanel onClose={vi.fn()} />);
  await screen.findByText(t().toolchainSdks);
  expect(screen.getByText(/dotnet: 8\.0\.100/)).toBeTruthy();
  expect(screen.getByText(new RegExp(`java: ${t().toolchainSdkAbsent}`))).toBeTruthy();
});

it("an older payload without sdks renders no SDK block", async () => {
  auditToolchains.mockResolvedValue({
    vswhere_available: true,
    instances: [],
    package_cache_kb: 0,
    package_cache_capped: false,
    notes: [],
    completeness: { status: "complete", roots_scanned: 1, failed_roots: [] },
  });
  render(<ToolchainPanel onClose={vi.fn()} />);
  await screen.findByText(t().toolchainNoInstances);
  expect(screen.queryByText(t().toolchainSdks)).toBeNull();
});
