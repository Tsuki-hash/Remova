/** Shared zh/en dictionary runtime for Remova UI. */
import { dict as zh } from "./zh";
import { dict as en } from "./en";

export type Lang = "zh" | "en";

const dict = { zh, en } as const;

let lang: Lang = "zh";

export function loadLang() {
  const v = localStorage.getItem("remova_lang");
  if (v === "en" || v === "zh") lang = v;
  return lang;
}

export function setLang(l: Lang) {
  lang = l;
  localStorage.setItem("remova_lang", l);
}

export function currentLang() {
  return lang;
}

export type Strings = (typeof zh);

// Compile-time: zh/en must expose the same keys.
type MissingInEn = Exclude<keyof typeof zh, keyof typeof en>;
type MissingInZh = Exclude<keyof typeof en, keyof typeof zh>;
const _i18nKeysMatch: [MissingInEn, MissingInZh] extends [never, never] ? true : never = true;
void _i18nKeysMatch;

export function formatSize(kb: number): string {
  if (!kb || kb <= 0) return "—";
  if (kb < 1024) return `${kb} KB`;
  const mb = kb / 1024;
  if (mb < 1024) return `${mb.toFixed(1)} MB`;
  return `${(mb / 1024).toFixed(2)} GB`;
}

export function t(): Strings {
  return dict[lang] as Strings;
}

export { dict };
