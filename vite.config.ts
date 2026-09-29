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
      // Logic + App orchestration gate (R23-QA-05). Components are exercised by
      // smoke tests; api.ts is thin invoke wrappers whose contract is checked
      // by apiContract.test, not by execution.
      include: ["src/lib/**", "src/hooks/**", "src/App.tsx"],
      exclude: ["src/lib/api.ts"],
      thresholds: {
        // Preserve the existing logic-layer floor; App gets its own gate so
        // better hook coverage cannot mask missing orchestration tests.
        statements: 48,
        branches: 49,
        functions: 44,
        lines: 49,
        "src/App.tsx": {
          statements: 90,
          branches: 85,
          functions: 90,
          lines: 90,
        },
      },
    },
  },
});
