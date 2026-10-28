// Anonymous ranking: where to send results and how to read the ranking.

let base: Promise<string | null> | null = null;

/** The API address, published by the deploy workflow in api.json. */
export function apiBase(): Promise<string | null> {
  base ??= fetch(new URL("./api.json", location.href).href, { cache: "no-cache" })
    .then((r) => (r.ok ? r.json() : null))
    .then((j: { base?: string | null } | null) => j?.base ?? null)
    .catch(() => null);
  return base;
}

export interface SubmitResult {
  ok: boolean;
  org?: string;
  country?: string;
}

export async function submitResult(r: { downMbps: number; uploadMbps: number; idleMs: number | null; loadedMs: number | null }): Promise<SubmitResult | null> {
  const b = await apiBase();
  if (!b) return null;
  try {
    const res = await fetch(`${b}/submit`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ downMbps: r.downMbps, upMbps: r.uploadMbps, idleMs: r.idleMs, loadedMs: r.loadedMs, source: "web" }),
    });
    return (await res.json()) as SubmitResult;
  } catch {
    return null;
  }
}

export interface Operator {
  asn: number;
  org: string;
  results: number;
  downMbps: number;
  upMbps: number;
  idleMs: number | null;
  lagMs: number | null;
  laggyPct: number | null;
}

export interface Ranking {
  ok: boolean;
  country: string;
  days: number;
  minResults: number;
  total: number;
  updated: number | null;
  operators: Operator[];
}

export async function fetchRanking(country?: string): Promise<Ranking | null> {
  const b = await apiBase();
  if (!b) return null;
  try {
    const res = await fetch(`${b}/ranking${country ? `?country=${encodeURIComponent(country)}` : ""}`);
    return (await res.json()) as Ranking;
  } catch {
    return null;
  }
}
