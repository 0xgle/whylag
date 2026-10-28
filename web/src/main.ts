import "@fontsource/barlow/400.css";
import "@fontsource/barlow/500.css";
import "@fontsource/barlow/600.css";
import "@fontsource/barlow-semi-condensed/600.css";
import "@fontsource/barlow-semi-condensed/700.css";
import "./style.css";
import { detectLang, tr, type Key, type Lang } from "./i18n";
import { runDemo, runTest, type Progress, type Result } from "./ndt7";
import { apiBase, submitResult } from "./ranking";

let lang: Lang = detectLang();
const demo = new URLSearchParams(location.search).has("demo");
const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

function applyLang() {
  document.documentElement.lang = lang;
  document.querySelectorAll<HTMLElement>("[data-t]").forEach((el) => {
    el.textContent = tr(lang, el.dataset.t as Key);
  });
  document.querySelectorAll<HTMLButtonElement>("[data-lang]").forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.lang === lang)));
  document.title = `WhyLag — ${tr(lang, "title")}`;
  if (last) renderResult(last);
}

const fmt = (v: number | null | undefined, digits = 0) =>
  v == null || !Number.isFinite(v) ? "—" : new Intl.NumberFormat(lang === "pl" ? "pl-PL" : "en-GB", { maximumFractionDigits: digits }).format(v);

function bloatGrade(idle: number, loaded: number) {
  const extra = Math.max(0, loaded - idle);
  if (extra < 5) return "A+";
  if (extra < 30) return "A";
  if (extra < 60) return "B";
  if (extra < 200) return "C";
  if (extra < 400) return "D";
  return "F";
}

const ORDER = ["A", "B", "C", "D", "F"];
const COLORS: Record<string, string> = { A: "#12805a", B: "#3fa34d", C: "#c99a12", D: "#d9632b", F: "#c0263d" };

/** Overall grade: lag under load first, then step down for slow lines. */
function overall(r: Result) {
  const loaded = Math.max(r.loadedDownMs ?? 0, r.loadedUpMs ?? 0);
  const bloat = r.idleMs != null && loaded ? bloatGrade(r.idleMs, loaded) : "B";
  let i = ORDER.indexOf(bloat === "A+" ? "A" : bloat);
  if (r.downloadMbps < 25) i += 1;
  if (r.downloadMbps < 10) i += 1;
  const grade = ORDER[Math.min(ORDER.length - 1, i)];
  const bloated = ["C", "D", "F"].includes(bloat);
  const slow = r.downloadMbps < 25;
  return { grade, bloat, bloated, slow, loaded };
}

function onProgress(p: Progress) {
  $("fill").style.width = `${Math.round(p.progress * 100)}%`;
  $("phase").textContent = tr(lang, p.phase === "locate" ? "phaseLocate" : p.phase === "download" ? "phaseDownload" : p.phase === "upload" ? "phaseUpload" : "phaseDone");
  $("down").textContent = fmt(p.downloadMbps);
  $("up").textContent = fmt(p.uploadMbps, p.uploadMbps != null && p.uploadMbps < 10 ? 1 : 0);
  const loaded = Math.max(p.loadedDownMs ?? 0, p.loadedUpMs ?? 0);
  $("lag").textContent = loaded ? fmt(loaded) : "—";
  $("idle").textContent = p.idleMs != null ? `${fmt(p.idleMs)} ms ${tr(lang, "idle")}` : "";
}

let last: Result | null = null;

function renderResult(r: Result) {
  const o = overall(r);
  const tile = $("grade");
  tile.textContent = o.grade;
  tile.style.background = COLORS[o.grade];
  $("verdict").textContent = tr(lang, o.bloated && o.slow ? "verdictBoth" : o.bloated ? "verdictBloat" : o.slow ? "verdictSlow" : "verdictGood");
  const tips: string[] = [];
  if (o.bloated) tips.push(tr(lang, "tipBloat", { idle: fmt(r.idleMs), loaded: fmt(o.loaded) }));
  if (o.slow) tips.push(tr(lang, "tipSlow", { down: fmt(r.downloadMbps) }));
  tips.push(tr(lang, "tipWifi"));
  $("tips").replaceChildren(
    ...tips.map((t) => {
      const li = document.createElement("li");
      li.textContent = t;
      return li;
    }),
  );
  $("server").textContent = tr(lang, "server", { server: r.server });
  $("result").hidden = false;
}

async function shareCard(r: Result) {
  await document.fonts?.ready;
  const o = overall(r);
  const c = document.createElement("canvas");
  c.width = 1200;
  c.height = 630;
  const x = c.getContext("2d")!;
  x.fillStyle = "#edf0f3";
  x.fillRect(0, 0, 1200, 630);
  x.fillStyle = COLORS[o.grade];
  x.beginPath();
  x.roundRect(70, 120, 300, 300, 36);
  x.fill();
  x.fillStyle = "#fff";
  x.font = "700 230px 'Barlow Semi Condensed', sans-serif";
  x.textAlign = "center";
  x.textBaseline = "middle";
  x.fillText(o.grade, 220, 282);
  x.textAlign = "left";
  x.textBaseline = "alphabetic";
  x.fillStyle = "#4a5563";
  x.font = "600 30px Barlow, sans-serif";
  x.fillText(tr(lang, "grade"), 420, 150);
  x.fillStyle = "#17202b";
  x.font = "600 54px 'Barlow Semi Condensed', sans-serif";
  const words = $("verdict").textContent!.split(" ");
  let line = "";
  let y = 220;
  for (const w of words) {
    if (x.measureText(`${line} ${w}`).width > 700 && line) {
      x.fillText(line, 420, y);
      y += 60;
      line = w;
    } else line = line ? `${line} ${w}` : w;
  }
  x.fillText(line, 420, y);
  const stats = [
    [tr(lang, "download"), `${fmt(r.downloadMbps)} Mb/s`],
    [tr(lang, "upload"), `${fmt(r.uploadMbps)} Mb/s`],
    [tr(lang, "lag"), `${fmt(o.loaded)} ms`],
  ];
  stats.forEach(([label, value], i) => {
    x.fillStyle = "#7d8794";
    x.font = "500 24px Barlow, sans-serif";
    x.fillText(label, 420 + i * 240, y + 80);
    x.fillStyle = "#17202b";
    x.font = "700 40px 'Barlow Semi Condensed', sans-serif";
    x.fillText(value, 420 + i * 240, y + 126);
  });
  x.fillStyle = "#2433c9";
  x.font = "700 32px 'Barlow Semi Condensed', sans-serif";
  x.fillText("WhyLag", 70, 560);
  x.fillStyle = "#7d8794";
  x.font = "500 24px Barlow, sans-serif";
  x.textAlign = "right";
  x.fillText(location.host || "whylag", 1130, 560);
  const blob: Blob = await new Promise((res) => c.toBlob((b) => res(b!), "image/png"));
  const file = new File([blob], "whylag.png", { type: "image/png" });
  const text = tr(lang, "shareText", { grade: o.grade });
  if (navigator.canShare?.({ files: [file] })) {
    try {
      await navigator.share({ files: [file], text, url: location.href.split("?")[0] });
      return;
    } catch {
      /* cancelled: fall back to download */
    }
  }
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = "whylag.png";
  a.click();
}

const SHARE_KEY = "whylag.shareRanking";

async function shareToRanking(r: Result) {
  const loaded = Math.max(r.loadedDownMs ?? 0, r.loadedUpMs ?? 0) || null;
  const res = await submitResult({ downMbps: r.downloadMbps, uploadMbps: r.uploadMbps, idleMs: r.idleMs, loadedMs: loaded });
  if (!res?.ok || !res.org) return;
  const p = $("ranked");
  const link = document.createElement("a");
  link.href = `./ranking.html${res.country ? `?country=${res.country}` : ""}`;
  link.textContent = tr(lang, "rankingLink");
  p.replaceChildren(document.createTextNode(`${tr(lang, "rankingAdded", { org: res.org })} `), link);
  p.hidden = false;
}

async function start() {
  const btn = $<HTMLButtonElement>("start");
  btn.disabled = true;
  $("error").hidden = true;
  $("result").hidden = true;
  $("ranked").hidden = true;
  try {
    last = await (demo ? runDemo : runTest)(onProgress);
    renderResult(last);
    if (!demo && $<HTMLInputElement>("shareRanking").checked) void shareToRanking(last);
    $("result").scrollIntoView({ behavior: "smooth", block: "center" });
  } catch {
    $("error").textContent = tr(lang, "failed");
    $("error").hidden = false;
  } finally {
    btn.disabled = false;
  }
}

document.querySelectorAll<HTMLButtonElement>("[data-lang]").forEach((b) =>
  b.addEventListener("click", () => {
    lang = b.dataset.lang as Lang;
    applyLang();
  }),
);
$("start").addEventListener("click", start);
// The ranking options appear only once the ranking API is deployed.
void apiBase().then((b) => {
  if (!b) return;
  $("shareOpt").hidden = false;
  $("rankNav").hidden = false;
});
try {
  if (localStorage.getItem(SHARE_KEY) === "0") $<HTMLInputElement>("shareRanking").checked = false;
} catch {
  /* storage unavailable */
}
$("shareRanking").addEventListener("change", (e) => {
  try {
    localStorage.setItem(SHARE_KEY, (e.target as HTMLInputElement).checked ? "1" : "0");
  } catch {
    /* storage unavailable */
  }
});
$("again").addEventListener("click", start);
$("share").addEventListener("click", () => last && shareCard(last));
applyLang();
