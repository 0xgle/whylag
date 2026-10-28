import { fmtNumber, type Lang } from "../i18n";

/** Formats translation params: numbers rounded, arrays joined. */
export function fmtParams(lang: Lang, params: Record<string, unknown>): Record<string, string | number> {
  const out: Record<string, string | number> = {};
  for (const [k, v] of Object.entries(params ?? {})) {
    if (typeof v === "number") out[k] = Math.abs(v) >= 10 || Number.isInteger(v) ? fmtNumber(lang, v) : fmtNumber(lang, v, 1);
    else if (Array.isArray(v)) out[k] = v.join(", ");
    else if (v == null) out[k] = "—";
    else out[k] = String(v);
  }
  return out;
}

export function fmtTime(lang: Lang, ms: number, withDate = false) {
  return new Intl.DateTimeFormat(lang === "pl" ? "pl-PL" : "en-GB", {
    hour: "2-digit",
    minute: "2-digit",
    second: withDate ? undefined : "2-digit",
    ...(withDate ? { day: "numeric", month: "short" } : {}),
  }).format(new Date(ms));
}

export function fmtDateTime(lang: Lang, ms: number) {
  return new Intl.DateTimeFormat(lang === "pl" ? "pl-PL" : "en-GB", { dateStyle: "medium", timeStyle: "short" }).format(new Date(ms));
}

export function fmtDuration(ms: number) {
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} min ${s % 60 ? `${s % 60} s` : ""}`.trim();
  return `${Math.floor(m / 60)} h ${m % 60} min`;
}
