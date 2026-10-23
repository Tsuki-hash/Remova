/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import pkg from "./package.json";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  define: {
    __APP_VERSION__: JSON.stringify(pkg.version),
  },
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  test: {
    // Node CLI tests use their own runner (npm run test:scripts).
    include: ["src/**/*.test.{ts,tsx}"],
    coverage: {
      provider: "v8",
      reporter: ["text-summary", "text", "html"],
      // Logic + App orchestration gate. Components are exercised by
      // smoke tests; api.ts is thin invoke wrappers whose contract is checked
      // by apiContract.test, not by execution.
      include: [
        "src/lib/**",
        "src/hooks/**",
        "src/App.tsx",
        // 1.3.2 destructive panels: lock these paths to the coverage gate so
        // new cleanup/report actions cannot regress without a test.
        "src/components/BrokenInstallsPanel.tsx",
        "src/components/FirewallRulesPanel.tsx",
        "src/components/CleanupResultDetails.tsx",
        "src/components/DeepRegistryCheck.tsx",
        "src/components/SoftwarePage.tsx",
        "src/components/BatchPanels.tsx",
      ],
      exclude: ["src/lib/api.ts"],
      thresholds: {
        // Preserve the existing logic-layer floor; App gets its own gate so
        // better hook coverage cannot mask missing orchestration tests.
        statements: 80,
        branches: 75,
        functions: 85,
        lines: 85,
        // This per-file gate used to sit at 90/85/90/90, i.e. above the
        // project-wide floor *and* above what App.tsx actually measures today
        // (100 / 93.5 / 98.18 / 100). The effect was perverse: extracting a
        // coherent block out of a 765-line component moves its statements into a
        // new file that no test imports yet, so the correct refactor read as a
        // coverage regression and the gate argued for keeping the file big.
        // 85/80/85/85 still demands that the orchestration paths be exercised,
        // while leaving room to split the file in the direction the rest of the
        // codebase already goes (hooks under src/hooks, panels under
        // src/components). Re-measure after any split and record the delta.
        "src/App.tsx": {
          statements: 85,
          branches: 80,
          functions: 85,
          lines: 85,
        },
        // Real-component interaction tests now cover
        // cleanup, cancel, banners and the report-identity cache. The floor is
        // raised with ~4 points of headroom below the measured 47.6/69.1/24/51.8
        // so a small change does not turn CI red while regressions still do.
        "src/components/SoftwarePage.tsx": {
          statements: 42,
          branches: 62,
          functions: 21,
          lines: 45,
        },
      },
    },
  },
});
