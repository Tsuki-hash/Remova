// Minimal ambient shims — tsconfig.vitest pins types to ["vite/client"] and the
// repo intentionally does not ship @types/node. Only the two builtins the IPC
// contract test needs to read repo files are declared here.
declare module "node:fs" {
  export function readFileSync(p: URL | string, encoding: "utf8"): string;
}
