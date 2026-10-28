/**
 * WhyLag ranking API (Cloudflare Worker + D1).
 *
 *   POST /submit   anonymous speed/lag result from the web test or the app
 *   GET  /ranking?country=PL   operators ranked by lag under load
 *
 * Nothing that identifies a person is stored: no IP address, no ID, no cookie,
 * no city, and times are rounded to the hour. The operator (ASN) and country
 * come from Cloudflare's own lookup of the connection. The address is used in
 * memory for the rate limit and, as a hash with a salt that is deleted the
 * next day, for a daily cap of a few results per connection.
 */

interface Env {
  DB: D1Database;
  LIMITER?: { limit(opts: { key: string }): Promise<{ success: boolean }> };
  ALLOWED_ORIGINS: string;
  /** Local development only (.dev.vars): network data comes from a test header. */
  DEV_FAKE_CF?: string;
}

function cfOf(req: Request, env: Env): IncomingRequestCfProperties | undefined {
  if (env.DEV_FAKE_CF === "1") {
    const fake = req.headers.get("X-Test-Cf");
    if (fake) return JSON.parse(fake);
  }
  return (req as Request & { cf?: IncomingRequestCfProperties }).cf;
}

const DAYS = 30;
/** Results one connection may add per day. */
const DAILY_CAP = 3;
const HOUR = 3_600_000;
/** Operators with fewer results are left out: too few to judge. */
const MIN_RESULTS = 15;

export interface Submission {
  downMbps: number;
  upMbps: number;
  idleMs: number | null;
  loadedMs: number | null;
  source: "web" | "app";
}

function num(v: unknown, min: number, max: number): number | null {
  return typeof v === "number" && Number.isFinite(v) && v >= min && v <= max ? v : null;
}

/** Accepts only plausible numbers; anything else is rejected. */
export function validate(body: unknown): Submission | null {
  if (!body || typeof body !== "object") return null;
  const b = body as Record<string, unknown>;
  const downMbps = num(b.downMbps, 0.01, 20_000);
  const upMbps = num(b.upMbps, 0.01, 20_000);
  if (downMbps == null || upMbps == null) return null;
  const idleMs = b.idleMs == null ? null : num(b.idleMs, 0.1, 5_000);
  const loadedMs = b.loadedMs == null ? null : num(b.loadedMs, 0.1, 20_000);
  if ((b.idleMs != null && idleMs == null) || (b.loadedMs != null && loadedMs == null)) return null;
  const source = b.source === "app" ? "app" : "web";
  return { downMbps, upMbps, idleMs, loadedMs, source };
}

/** Shortens registry names ("Orange Polska Spolka Akcyjna" → "Orange Polska"). */
export function cleanOrg(org: string): string {
  return org
    .replace(/\b(spolka akcyjna|spółka akcyjna|sp\.? ?z ?o\.? ?o\.?|s\.a\.?|sa|gmbh|ag|ltd\.?|limited|inc\.?|llc|b\.v\.?|s\.r\.o\.?|plc|corporation|corp\.?)\b/gi, "")
    .replace(/[,.\s]+$/, "")
    .replace(/\s{2,}/g, " ")
    .trim();
}

/** IPv4 address, or the /64 network of an IPv6 address. */
export function submitterKey(ip: string): string {
  if (!ip.includes(":")) return ip;
  // Expand "::" so the first four groups are the network part.
  const [head, tail = ""] = ip.split("::");
  const h = head ? head.split(":") : [];
  const t = tail ? tail.split(":") : [];
  const groups = [...h, ...Array(Math.max(0, 8 - h.length - t.length)).fill("0"), ...t];
  return groups.slice(0, 4).map((g) => g.toLowerCase().replace(/^0+(?=.)/, "")).join(":") + "::/64";
}

const today = () => new Date().toISOString().slice(0, 10);

async function sha256(text: string) {
  const d = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return [...new Uint8Array(d)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/** Counts one submission against the connection's daily cap. */
async function takeQuota(env: Env, who: string): Promise<boolean> {
  const day = today();
  let salt = (await env.DB.prepare("SELECT value FROM salt WHERE day = ?").bind(day).first<{ value: string }>())?.value;
  if (!salt) {
    const fresh = [...crypto.getRandomValues(new Uint8Array(16))].map((b) => b.toString(16).padStart(2, "0")).join("");
    await env.DB.prepare("INSERT OR IGNORE INTO salt (day, value) VALUES (?, ?)").bind(day, fresh).run();
    salt = (await env.DB.prepare("SELECT value FROM salt WHERE day = ?").bind(day).first<{ value: string }>())!.value;
  }
  const key = await sha256(`${salt}|${who}`);
  const row = await env.DB.prepare("INSERT INTO quota (day, key, n) VALUES (?, ?, 1) ON CONFLICT(day, key) DO UPDATE SET n = n + 1 RETURNING n")
    .bind(day, key)
    .first<{ n: number }>();
  return (row?.n ?? 1) <= DAILY_CAP;
}

function cors(origin: string | null, env: Env): Record<string, string> {
  const allowed = env.ALLOWED_ORIGINS.split(",").map((s) => s.trim());
  const ok = origin && allowed.includes(origin);
  return {
    "Access-Control-Allow-Origin": ok ? origin : allowed[0],
    "Access-Control-Allow-Methods": "GET, POST, OPTIONS",
    "Access-Control-Allow-Headers": "Content-Type",
    "Access-Control-Max-Age": "86400",
    Vary: "Origin",
  };
}

function json(data: unknown, status: number, headers: Record<string, string>) {
  return new Response(JSON.stringify(data), { status, headers: { ...headers, "Content-Type": "application/json; charset=utf-8" } });
}

export interface OperatorRow {
  asn: number;
  org: string;
  results: number;
  downMbps: number;
  upMbps: number;
  idleMs: number | null;
  /** Extra delay while the line is busy (the lag people feel). */
  lagMs: number | null;
  /** Share of results with more than 100 ms extra lag under load. */
  laggyPct: number | null;
}

function median(v: number[]): number | null {
  if (!v.length) return null;
  const s = [...v].sort((a, b) => a - b);
  const m = s.length >> 1;
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
}

const round = (v: number | null, d = 0) => (v == null ? null : Math.round(v * 10 ** d) / 10 ** d);

/** Groups raw results per operator and ranks them, least lag first. */
export function rank(rows: { asn: number; org: string; down_mbps: number; up_mbps: number; idle_ms: number | null; loaded_ms: number | null }[]): OperatorRow[] {
  const by = new Map<number, typeof rows>();
  for (const r of rows) {
    if (!by.has(r.asn)) by.set(r.asn, []);
    by.get(r.asn)!.push(r);
  }
  const out: OperatorRow[] = [];
  for (const [asn, list] of by) {
    if (list.length < MIN_RESULTS) continue;
    const lags = list.filter((r) => r.idle_ms != null && r.loaded_ms != null).map((r) => Math.max(0, r.loaded_ms! - r.idle_ms!));
    out.push({
      asn,
      org: list[list.length - 1].org,
      results: list.length,
      downMbps: round(median(list.map((r) => r.down_mbps)))!,
      upMbps: round(median(list.map((r) => r.up_mbps)))!,
      idleMs: round(median(list.filter((r) => r.idle_ms != null).map((r) => r.idle_ms!))),
      lagMs: round(median(lags)),
      laggyPct: lags.length ? round((100 * lags.filter((l) => l > 100).length) / lags.length) : null,
    });
  }
  return out.sort((a, b) => (a.lagMs ?? 1e9) - (b.lagMs ?? 1e9) || b.downMbps - a.downMbps);
}

async function rebuild(env: Env) {
  const since = Date.now() - DAYS * 86_400_000;
  // Countries with recent results, plus any that still have a stored ranking
  // (so a country whose results all aged out gets an empty ranking).
  const countries = await env.DB.prepare("SELECT DISTINCT country FROM results WHERE ts >= ? UNION SELECT country FROM ranking").bind(since).all<{ country: string }>();
  for (const { country } of countries.results) {
    const rows = await env.DB.prepare("SELECT asn, org, down_mbps, up_mbps, idle_ms, loaded_ms FROM results WHERE country = ? AND ts >= ? ORDER BY ts")
      .bind(country, since)
      .all<{ asn: number; org: string; down_mbps: number; up_mbps: number; idle_ms: number | null; loaded_ms: number | null }>();
    const operators = rank(rows.results);
    const total = rows.results.length;
    await env.DB.prepare("INSERT INTO ranking (country, updated, json) VALUES (?, ?, ?) ON CONFLICT(country) DO UPDATE SET updated = excluded.updated, json = excluded.json")
      .bind(country, Date.now(), JSON.stringify({ country, days: DAYS, minResults: MIN_RESULTS, total, operators }))
      .run();
  }
  // Raw results older than the window are not needed any more, and yesterday's
  // salts and counters must go so the daily keys cannot be recomputed.
  await env.DB.prepare("DELETE FROM results WHERE ts < ?").bind(Date.now() - DAYS * 86_400_000).run();
  await env.DB.prepare("DELETE FROM quota WHERE day < ?").bind(today()).run();
  await env.DB.prepare("DELETE FROM salt WHERE day < ?").bind(today()).run();
}

export default {
  async fetch(req: Request, env: Env): Promise<Response> {
    const url = new URL(req.url);
    const headers = cors(req.headers.get("Origin"), env);
    if (req.method === "OPTIONS") return new Response(null, { status: 204, headers });

    if (req.method === "POST" && url.pathname === "/submit") {
      const ip = (env.DEV_FAKE_CF === "1" && req.headers.get("X-Test-Ip")) || req.headers.get("CF-Connecting-IP") || "";
      // One person on IPv6 controls a whole /64, so that is the unit.
      const who = submitterKey(ip);
      if (env.LIMITER && !(await env.LIMITER.limit({ key: who })).success) return json({ ok: false, error: "rateLimited" }, 429, headers);
      const body = await req.json().catch(() => null);
      const s = validate(body);
      if (!s) return json({ ok: false, error: "invalid" }, 400, headers);
      const cf = cfOf(req, env);
      const asn = Number(cf?.asn ?? 0);
      const country = String(cf?.country ?? "").toUpperCase();
      if (!asn || !/^[A-Z]{2}$/.test(country)) return json({ ok: false, error: "noNetwork" }, 400, headers);
      const org = cleanOrg(String(cf?.asOrganization ?? `AS${asn}`)) || `AS${asn}`;
      if (!(await takeQuota(env, who))) return json({ ok: false, error: "dailyLimit", country, org }, 429, headers);
      await env.DB.prepare("INSERT INTO results (ts, country, asn, org, city, source, down_mbps, up_mbps, idle_ms, loaded_ms) VALUES (?, ?, ?, ?, NULL, ?, ?, ?, ?, ?)")
        .bind(Math.floor(Date.now() / HOUR) * HOUR, country, asn, org, s.source, s.downMbps, s.upMbps, s.idleMs, s.loadedMs)
        .run();
      return json({ ok: true, country, org }, 200, headers);
    }

    if (req.method === "GET" && url.pathname === "/ranking") {
      const country = (url.searchParams.get("country") ?? cfOf(req, env)?.country ?? "PL").toString().toUpperCase();
      if (!/^[A-Z]{2}$/.test(country)) return json({ ok: false, error: "invalid" }, 400, headers);
      const row = await env.DB.prepare("SELECT updated, json FROM ranking WHERE country = ?").bind(country).first<{ updated: number; json: string }>();
      const data = row ? { ...JSON.parse(row.json), updated: row.updated } : { country, days: DAYS, minResults: MIN_RESULTS, total: 0, operators: [], updated: null };
      return json({ ok: true, ...data }, 200, { ...headers, "Cache-Control": "public, max-age=300" });
    }

    if (env.DEV_FAKE_CF === "1" && url.pathname === "/__rebuild") {
      await rebuild(env);
      return json({ ok: true }, 200, headers);
    }
    if (url.pathname === "/") return json({ ok: true, service: "whylag-api" }, 200, headers);
    return json({ ok: false, error: "notFound" }, 404, headers);
  },

  async scheduled(_event: ScheduledController, env: Env, ctx: ExecutionContext) {
    ctx.waitUntil(rebuild(env));
  },
};

export { rebuild };
