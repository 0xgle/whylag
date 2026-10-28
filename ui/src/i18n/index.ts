import { useCallback } from "react";
import { en, type Key } from "./en";
import { pl } from "./pl";
import { useApp } from "../store";

export type { Key };
export type Lang = "en" | "pl";
const dicts = { en, pl };

export function translate(lang: Lang, key: Key, vars?: Record<string, string | number>): string {
  let s: string = dicts[lang][key] ?? en[key] ?? key;
  if (vars) for (const [k, v] of Object.entries(vars)) s = s.replaceAll(`{${k}}`, String(v));
  return s;
}

/** Translates a dynamic key (e.g. built from a backend code); returns `fallback` if missing. */
export function translateMaybe(lang: Lang, key: string, fallback: string, vars?: Record<string, string | number>) {
  return key in en ? translate(lang, key as Key, vars) : fallback;
}

export function useT() {
  const lang = useApp((s) => s.lang);
  return useCallback((key: Key, vars?: Record<string, string | number>) => translate(lang, key, vars), [lang]);
}

export function useLang() {
  return useApp((s) => s.lang);
}

const numberFormats = new Map<string, Intl.NumberFormat>();
export function fmtNumber(lang: Lang, n: number, digits = 0) {
  const key = `${lang}:${digits}`;
  let f = numberFormats.get(key);
  if (!f) {
    f = new Intl.NumberFormat(lang === "pl" ? "pl-PL" : "en-GB", { maximumFractionDigits: digits, minimumFractionDigits: digits });
    numberFormats.set(key, f);
  }
  return f.format(n);
}
