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
    coverage: {
      provider: "v8",
      reporter: ["text-summary", "text", "html"],
      // Logic layer gate (REV-QA-09): lib + hooks. Components are exercised by
      // smoke tests; api.ts is thin invoke wrappers whose contract is checked
      // by apiContract.test, not by execution.
      include: ["src/lib/**", "src/hooks/**"],
      exclude: ["src/lib/api.ts"],
      thresholds: {
        // Baseline 2026-09-26 (lines 44.15 / stmts 43.18 / branches 46.97 /
        // functions 40.2) minus a small margin — tighten as hooks coverage grows.
        statements: 41,
        branches: 45,
        functions: 38,
        lines: 42,
      },
    },
  },
});
