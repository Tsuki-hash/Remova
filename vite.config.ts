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
        // 2026-09-26 (post batch: boot/controller/closeMode/theme/deep pipeline
        // tests): lines 51.3 / stmts 49.97 / branches 50.73 / funcs 46.15.
        // Tighten further as hooks coverage grows.
        statements: 48,
        branches: 49,
        functions: 44,
        lines: 49,
      },
    },
  },
});
