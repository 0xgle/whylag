import "@fontsource/barlow/400.css";
import "@fontsource/barlow/500.css";
import "@fontsource/barlow/600.css";
import "@fontsource/barlow-semi-condensed/600.css";
import "@fontsource/barlow-semi-condensed/700.css";
import "./style.css";
import { detectLang, tr, type Key, type Lang } from "./i18n";
import { fetchRanking, type Ranking } from "./ranking";

const COUNTRIES = ["PL", "DE", "GB", "US", "CZ", "SK", "UA", "LT", "NL", "FR", "ES", "IT", "AT", "IE", "SE", "NO", "DK", "FI", "CA", "AU"];

let lang: Lang = detectLang();
const params = new URLSearchParams(location.search);
let country = (params.get("country") ?? guessCountry()).toUpperCase();
let data: Ranking | null | undefined;
const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

function guessCountry() {
  if (lang === "pl") return "PL";
  const region = navigator.language?.split("-")[1];
  return region && region.length === 2 ? region : "US";
}

const fmt = (v: number | null, digits = 0) =>
  v == null ? "—" : new Intl.NumberFormat(lang === "pl" ? "pl-PL" : "en-GB", { maximumFractionDigits: digits }).format(v);

function countryName(code: string) {
  try {
    return new Intl.DisplayNames([lang], { type: "region" }).of(code) ?? code;
  } catch {
    return code;
  }
}

function lagClass(ms: number | null) {
  if (ms == null) return "";
  return ms < 30 ? "good" : ms < 80 ? "fair" : "bad";
}

function render() {
  document.documentElement.lang = lang;
  document.querySelectorAll<HTMLElement>("[data-t]").forEach((el) => (el.textContent = tr(lang, el.dataset.t as Key)));
  document.querySelectorAll<HTMLButtonElement>("[data-lang]").forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.lang === lang)));
  document.title = `WhyLag — ${tr(lang, "rankTitle")}`;
  $("lead").textContent = tr(lang, "rankLead", { days: data?.days ?? 30 });

  const select = $<HTMLSelectElement>("country");
  const list = COUNTRIES.includes(country) ? COUNTRIES : [country, ...COUNTRIES];
  select.replaceChildren(
    ...list
      .map((c) => ({ c, name: countryName(c) }))
      .sort((a, b) => a.name.localeCompare(b.name, lang))
      .map(({ c, name }) => {
        const o = document.createElement("option");
        o.value = c;
        o.textContent = name;
        o.selected = c === country;
        return o;
      }),
  );

  const empty = $("empty");
  const table = $("table");
  if (data === undefined) return;
  if (data === null) {
    table.hidden = true;
    empty.hidden = false;
    empty.textContent = tr(lang, "rankUnavailable");
    $("updated").textContent = "";
    return;
  }
  $("updated").textContent = data.updated
    ? tr(lang, "rankUpdated", {
        time: new Date(data.updated).toLocaleString(lang, { dateStyle: "medium", timeStyle: "short" }),
        days: data.days,
        total: fmt(data.total),
        min: data.minResults,
      })
    : "";
  if (!data.operators.length) {
    table.hidden = true;
    empty.hidden = false;
    empty.textContent = tr(lang, "rankEmpty");
    return;
  }
  empty.hidden = true;
  table.hidden = false;
  $("rows").replaceChildren(
    ...data.operators.map((o, i) => {
      const tr_ = document.createElement("tr");
      const cells: [string, string][] = [
        ["num", String(i + 1)],
        ["org", o.org],
        [`num lag ${lagClass(o.lagMs)}`, o.lagMs == null ? "—" : `${fmt(o.lagMs)} ms`],
        ["num", o.laggyPct == null ? "—" : `${fmt(o.laggyPct)}%`],
        ["num", `${fmt(o.downMbps)} Mb/s`],
        ["num", `${fmt(o.upMbps)} Mb/s`],
        ["num", o.idleMs == null ? "—" : `${fmt(o.idleMs)} ms`],
        ["num muted", fmt(o.results)],
      ];
      for (const [cls, text] of cells) {
        const td = document.createElement("td");
        td.className = cls;
        td.textContent = text;
        tr_.append(td);
      }
      return tr_;
    }),
  );
}

async function load() {
  data = undefined;
  render();
  data = await fetchRanking(country);
  render();
}

document.querySelectorAll<HTMLButtonElement>("[data-lang]").forEach((b) =>
  b.addEventListener("click", () => {
    lang = b.dataset.lang as Lang;
    render();
  }),
);
$("country").addEventListener("change", (e) => {
  country = (e.target as HTMLSelectElement).value;
  const u = new URL(location.href);
  u.searchParams.set("country", country);
  history.replaceState(null, "", u);
  void load();
});
void load();
