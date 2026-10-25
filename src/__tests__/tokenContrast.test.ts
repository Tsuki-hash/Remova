// @vitest-environment node
// Contrast gate: text-grade tokens must hold >= 4.5:1 against the surfaces
// they are drawn on, in both themes. Values are parsed straight out of
// theme.ts so the test can never drift from the shipped tokens.
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

type Tokens = Record<string, string>;

function tokensOf(branch: "dark" | "light"): Tokens {
  // Repo-relative via import.meta.url — no node globals needed in the test tsconfig.
  const src = readFileSync(new URL("../lib/theme.ts", import.meta.url), "utf8");
  const start = src.indexOf(`if (theme === "dark") {`);
  const elseAt = src.indexOf("} else {", start);
  const end = src.indexOf("localStorage.setItem", elseAt);
  const chunk = branch === "dark" ? src.slice(start, elseAt) : src.slice(elseAt, end);
  const out: Tokens = {};
  for (const m of chunk.matchAll(/--([a-z0-9-]+)",\s*"(#[0-9A-Fa-f]{6}|none)"/g)) {
    out[m[1]!] = m[2]!;
  }
  return out;
}

function luminance(hex: string): number {
  const v = hex.replace("#", "");
  const rgb = [0, 2, 4].map((i) => {
    const c = parseInt(v.slice(i, i + 2) ?? "00", 16) / 255;
    return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
  });
  return 0.2126 * rgb[0]! + 0.7152 * rgb[1]! + 0.0722 * rgb[2]!;
}

function contrast(a: string, b: string): number {
  const la = luminance(a);
  const lb = luminance(b);
  const [hi, lo] = la >= lb ? [la, lb] : [lb, la];
  return (hi + 0.05) / (lo + 0.05);
}

describe("token contrast (WCAG 1.4.3)", () => {
  const branches: Array<[string, Tokens]> = [
    ["dark", tokensOf("dark")],
    ["light", tokensOf("light")],
  ];

  for (const [name, tokens] of branches) {
    it(`${name}: body and muted text meet 4.5:1 on surfaces`, () => {
      expect(tokens.fg).toBeTruthy();
      expect(tokens.muted).toBeTruthy();
      expect(tokens.surface).toBeTruthy();
      expect(tokens["surface-2"]).toBeTruthy();
      for (const fg of [tokens.fg, tokens.muted]) {
        for (const bg of [tokens.surface, tokens["surface-2"], tokens.bg]) {
          expect(contrast(fg!, bg!)).toBeGreaterThanOrEqual(4.5);
        }
      }
    });

    it(`${name}: text-grade status tokens meet 4.5:1 on surfaces`, () => {
      for (const key of ["warn-ink", "ok-ink", "accent-text", "danger-text"]) {
        const fg = tokens[key];
        expect(fg, key).toBeTruthy();
        for (const bg of [tokens.surface, tokens["surface-2"], tokens.bg]) {
          expect(contrast(fg!, bg!), `${key} on ${bg}`).toBeGreaterThanOrEqual(4.5);
        }
      }
    });
  }
});
