import { fmtNumber, translate, translateMaybe, type Key, type Lang } from "../../i18n";
import type { LocatedAp, Diagnosis, DiscoveryRun, MonitorStatus, PathRun, PerfRun, SampleSeries, ScanResult, SecurityReport, Settings } from "../../types";
import { fmtDateTime, fmtDuration, fmtParams } from "../../lib/format";
import { INCIDENT_WHERE } from "../monitor/MonitorModule";

export type SectionId = "summary" | "wifi" | "survey" | "discovery" | "performance" | "path" | "security" | "monitor";

export interface ReportData {
  lang: Lang;
  client: string;
  sections: SectionId[];
  settings: Settings | null;
  diag: Diagnosis | null;
  scan: ScanResult | null;
  survey: {
    name: string;
    floors: {
      name: string;
      mode: "measured" | "predicted";
      images: { layer: string; png: string; coverage: number | null }[];
      aps: LocatedAp[];
      samples: number;
      areaM2: number | null;
    }[];
  } | null;
  discovery: DiscoveryRun | null;
  perf: PerfRun | null;
  path: PathRun | null;
  security: SecurityReport | null;
  monitor: { status: MonitorStatus; series: SampleSeries } | null;
}

const esc = (s: unknown) =>
  String(s ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");

const GRADE: Record<string, string> = { A: "#12805a", B: "#3fa34d", C: "#c99a12", D: "#d9632b", F: "#c0263d" };
const LEVEL: Record<string, string> = { critical: "#c0263d", high: "#d9632b", warning: "#c99a12", medium: "#c99a12", info: "#2433c9", low: "#2433c9", good: "#3fa34d" };

function lineChart(series: { values: (number | null)[]; color: string; label: string }[], ts: number[], lang: Lang, unit: string): string {
  const W = 680;
  const H = 180;
  const pad = { l: 40, r: 10, t: 10, b: 24 };
  const all = series.flatMap((s) => s.values.filter((v): v is number => v != null));
  if (!all.length || ts.length < 2) return "";
  const max = Math.max(...all) * 1.1 || 1;
  const x = (i: number) => pad.l + (i / (ts.length - 1)) * (W - pad.l - pad.r);
  const y = (v: number) => pad.t + (1 - v / max) * (H - pad.t - pad.b);
  const paths = series
    .map((s) => {
      let d = "";
      s.values.forEach((v, i) => {
        if (v == null) return;
        d += `${d && s.values[i - 1] != null ? "L" : "M"}${x(i).toFixed(1)},${y(v).toFixed(1)}`;
      });
      return `<path d="${d}" fill="none" stroke="${s.color}" stroke-width="1.4"/>`;
    })
    .join("");
  const ticks = [0, 0.5, 1].map((f) => {
    const v = max * f;
    return `<text x="${pad.l - 6}" y="${y(v) + 4}" text-anchor="end" font-size="10" fill="#7d8794">${fmtNumber(lang, v)}</text><line x1="${pad.l}" x2="${W - pad.r}" y1="${y(v)}" y2="${y(v)}" stroke="#e3e7ec"/>`;
  });
  const times = [0, Math.floor(ts.length / 2), ts.length - 1].map((i) => `<text x="${x(i)}" y="${H - 6}" text-anchor="middle" font-size="10" fill="#7d8794">${esc(fmtDateTime(lang, ts[i]))}</text>`);
  const legend = series.map((s) => `<span><i style="background:${s.color}"></i>${esc(s.label)}</span>`).join("");
  return `<svg viewBox="0 0 ${W} ${H}" width="100%">${ticks.join("")}${paths}${times.join("")}<text x="4" y="12" font-size="10" fill="#7d8794">${unit}</text></svg><div class="legend">${legend}</div>`;
}

export function buildReport(d: ReportData): string {
  const t = (k: Key, v?: Record<string, string | number>) => translate(d.lang, k, v);
  const tm = (k: string, fb: string, v?: Record<string, string | number>) => translateMaybe(d.lang, k, fb, v);
  const n = (v: number | null | undefined, digits = 0) => (v == null ? "—" : fmtNumber(d.lang, v, digits));
  const now = Date.now();
  const company = d.settings?.reportCompany || "";
  const author = d.settings?.reportAuthor || "";
  const out: string[] = [];
  const sectionTitle = (id: SectionId) => t(`reports.section.${id}` as Key);
  const has = (id: SectionId) => d.sections.includes(id);

  // Cover
  out.push(`<section class="cover">
    <div class="brand">${esc(company || "WhyLag")}</div>
    <h1>${esc(t("report.cover.title"))}</h1>
    ${d.client ? `<p class="site">${esc(t("report.cover.site"))}: <b>${esc(d.client)}</b></p>` : ""}
    <p class="meta">${esc(t("reports.generated", { date: fmtDateTime(d.lang, now) }))}${author ? `<br>${esc(t("reports.preparedBy", { name: author }))}` : ""}</p>
    ${
      d.diag && d.diag.verdict !== "notChecked"
        ? `<div class="verdict"><div class="grade" style="background:${GRADE[d.diag.grade] ?? "#2433c9"}">${esc(d.diag.grade)}</div><div><div class="small">${esc(t("overview.grade"))}</div><div class="verdict-text">${esc(t(`verdict.${d.diag.verdict}` as Key))}</div></div></div>`
        : ""
    }
    <div class="toc"><b>${esc(t("report.toc"))}</b><ol>${d.sections.map((s) => `<li>${esc(sectionTitle(s))}</li>`).join("")}</ol></div>
  </section>`);

  if (has("summary") && d.diag) {
    const items = d.diag.advice
      .map((a) => {
        const p = fmtParams(d.lang, a.params);
        const fix = tm(`advice.${a.code}.fix`, "", p);
        return `<div class="finding" style="border-color:${LEVEL[a.level]}"><b>${esc(tm(`advice.${a.code}.title`, a.code, p))}</b>${fix ? `<p>${esc(fix)}</p>` : ""}</div>`;
      })
      .join("");
    out.push(`<section><h2>${esc(sectionTitle("summary"))}</h2><h3>${esc(t("report.findings"))}</h3>${items}</section>`);
  }

  if (has("wifi")) {
    const conn = d.scan?.networks.find((x) => x.connected);
    let body = `<p class="muted">${esc(t("reports.noData"))}</p>`;
    if (d.scan) {
      const rows = d.scan.networks
        .slice(0, 40)
        .map(
          (x) =>
            `<tr${x.connected ? ' class="hl"' : ""}><td>${esc(x.hidden ? t("common.hidden") : x.ssid)}</td><td class="mono">${esc(x.bssid)}</td><td>${esc(x.vendor ?? "")}</td><td class="num">${x.rssi}</td><td class="num">${x.channel}</td><td class="num">${x.band ?? ""}</td><td class="num">${x.widthMhz}</td><td>${esc(t(`security.${x.security}` as Key))}</td></tr>`,
        )
        .join("");
      body = `${conn ? `<p>${esc(t("report.wifi.connected", { ssid: conn.ssid, channel: conn.channel, band: conn.band ?? "", rssi: conn.rssi }))}</p>` : ""}
        <p>${esc(t("report.wifi.networks", { n: d.scan.networks.length }))}</p>
        <table><thead><tr><th>${esc(t("wifi.col.ssid"))}</th><th>BSSID</th><th>${esc(t("wifi.col.vendor"))}</th><th class="num">dBm</th><th class="num">${esc(t("wifi.col.channel"))}</th><th class="num">GHz</th><th class="num">MHz</th><th>${esc(t("wifi.col.security"))}</th></tr></thead><tbody>${rows}</tbody></table>`;
    }
    out.push(`<section><h2>${esc(sectionTitle("wifi"))}</h2>${body}</section>`);
  }

  if (has("survey")) {
    let body = `<p class="muted">${esc(t("reports.noData"))}</p>`;
    if (d.survey && d.survey.floors.length) {
      const many = d.survey.floors.length > 1;
      body = d.survey.floors
        .map((f) => {
          const facts = [
            f.mode === "measured" ? t("report.survey.samples", { n: f.samples }) : t("report.survey.predicted"),
            f.areaM2 != null ? t("report.survey.area", { area: n(f.areaM2) }) : null,
          ].filter(Boolean);
          const figures = f.images
            .map(
              (img) =>
                `<figure><img src="${img.png}" alt=""><figcaption>${esc(t("report.survey.layer", { layer: t(`survey.layer.${img.layer}` as Key) }))}${
                  img.coverage != null && (img.layer === "signal" || img.layer === "secondSignal")
                    ? ` — ${esc(t("report.survey.coverage", { pct: n(img.coverage * 100), threshold: d.settings?.coverageThresholdDbm ?? -67 }))}`
                    : ""
                }</figcaption></figure>`,
            )
            .join("");
          const aps = f.aps.length
            ? `<h4>${esc(t("report.survey.aps"))}</h4><table><thead><tr><th>${esc(t("wifi.col.ssid"))}</th><th>BSSID</th><th class="num">${esc(t("wifi.col.channel"))}</th><th class="num">${esc(t("report.survey.strongest"))}</th><th>${esc(t("report.survey.position"))}</th></tr></thead><tbody>${f.aps
                .map(
                  (a) =>
                    `<tr><td>${esc(a.ssids.join(", ") || t("common.hidden"))}</td><td class="mono">${esc(a.bssids.slice(0, 3).join(" "))}${a.bssids.length > 3 ? " …" : ""}</td><td class="num">${esc(a.channels.join(", "))}</td><td class="num">${a.maxRssi} dBm</td><td>${esc(t(a.confidence === "high" ? "report.survey.posHigh" : "report.survey.posLow"))}</td></tr>`,
                )
                .join("")}</tbody></table>`
            : "";
          return `${many ? `<h3>${esc(f.name)}</h3>` : ""}<p>${esc(facts.join(" · "))}</p>${figures}${aps}`;
        })
        .join("");
    }
    out.push(`<section><h2>${esc(sectionTitle("survey"))}${d.survey ? `: ${esc(d.survey.name)}` : ""}</h2>${body}</section>`);
  }

  if (has("discovery")) {
    let body = `<p class="muted">${esc(t("reports.noData"))}</p>`;
    if (d.discovery && d.discovery.hosts.length) {
      const rows = d.discovery.hosts
        .map(
          (h) =>
            `<tr><td>${esc(h.name ?? "")}</td><td>${esc(t(`kind.${h.kind}` as Key))}</td><td class="mono">${esc(h.ip)}</td><td class="mono">${esc(h.mac ?? "")}</td><td>${esc(h.vendor ?? "")}</td><td>${esc(h.model ?? h.os ?? "")}</td><td>${esc(h.ports.map((p) => p.port).join(", "))}</td></tr>`,
        )
        .join("");
      body = `<p>${esc(t("report.discovery.total", { n: d.discovery.hosts.length, subnet: d.discovery.subnet ?? "" }))}</p>
        <table><thead><tr><th>${esc(t("discovery.col.device"))}</th><th>${esc(t("discovery.col.type"))}</th><th>IP</th><th>MAC</th><th>${esc(t("wifi.col.vendor"))}</th><th>${esc(t("discovery.detail.model"))}</th><th>${esc(t("discovery.col.ports"))}</th></tr></thead><tbody>${rows}</tbody></table>`;
    }
    out.push(`<section><h2>${esc(sectionTitle("discovery"))}</h2>${body}</section>`);
  }

  if (has("performance")) {
    let body = `<p class="muted">${esc(t("reports.noData"))}</p>`;
    if (d.perf && (d.perf.latency.length || d.perf.speed)) {
      const sp = d.perf.speed;
      const lat = d.perf.latency
        .map(
          (l) =>
            `<tr><td>${esc(l.id === "gateway" ? t("perf.latency.gateway") : l.label)}</td><td class="mono">${esc(l.host)}</td><td class="num">${n(l.avg, 1)}</td><td class="num">${n(l.p95, 1)}</td><td class="num">${n(l.jitter, 1)}</td><td class="num">${n(l.lossPct)}%</td></tr>`,
        )
        .join("");
      const dns = d.perf.dns.map((x) => `<tr><td>${esc(x.label)}${x.system ? ` (${esc(t("perf.dns.yours"))})` : ""}</td><td class="mono">${esc(x.server)}</td><td class="num">${n(x.medianMs, 1)}</td></tr>`).join("");
      body = `${sp ? `<p class="big">${esc(t("report.perf.speed", { down: n(sp.downloadMbps), up: n(sp.uploadMbps), grade: sp.grade ?? "—" }))}</p><p class="muted">${esc(t("perf.speed.server", { server: sp.server }))}. ${esc(t("perf.bloat.idle"))}: ${n(sp.idleLatencyMs)} ms, ${esc(t("perf.bloat.down"))}: ${n(sp.loadedDownLatencyMs)} ms, ${esc(t("perf.bloat.up"))}: ${n(sp.loadedUpLatencyMs)} ms.</p>` : ""}
        ${lat ? `<h3>${esc(t("perf.latency.title"))}</h3><table><thead><tr><th>${esc(t("perf.latency.target"))}</th><th>Host</th><th class="num">${esc(t("perf.latency.avg"))} (ms)</th><th class="num">${esc(t("perf.latency.p95"))}</th><th class="num">${esc(t("perf.latency.jitter"))}</th><th class="num">${esc(t("perf.latency.loss"))}</th></tr></thead><tbody>${lat}</tbody></table>` : ""}
        ${dns ? `<h3>${esc(t("perf.dns.title"))}</h3><table><thead><tr><th>${esc(t("perf.dns.server"))}</th><th>IP</th><th class="num">${esc(t("perf.dns.median"))} (ms)</th></tr></thead><tbody>${dns}</tbody></table>` : ""}`;
    }
    out.push(`<section><h2>${esc(sectionTitle("performance"))}</h2>${body}</section>`);
  }

  if (has("path")) {
    let body = `<p class="muted">${esc(t("reports.noData"))}</p>`;
    if (d.path && d.path.hops.length) {
      const rows = d.path.hops
        .map(
          (h) =>
            `<tr><td class="num">${h.ttl}</td><td>${esc(h.name ?? h.ip ?? t("path.noReply"))}</td><td class="mono">${esc(h.ip ?? "")}</td><td>${esc(t(`segment.${h.segment}` as Key))}</td><td class="num">${n(h.lossPct)}%</td><td class="num">${n(h.avg, 1)}</td><td class="num">${n(h.worst, 1)}</td></tr>`,
        )
        .join("");
      const findings = d.path.findings
        .map((f) => `<li>${esc(tm(`finding.${f.code}`, f.code, { ttl: f.ttl ?? "", value: f.value != null ? n(f.value) : "", segment: f.segment ? t(`segment.${f.segment}` as Key).toLowerCase() : "" }))}</li>`)
        .join("");
      body = `<h3>${esc(t("report.path.to", { target: d.path.target }))}</h3><ul>${findings}</ul><table><thead><tr><th class="num">#</th><th>${esc(t("path.col.host"))}</th><th>IP</th><th>${esc(t("path.col.where"))}</th><th class="num">${esc(t("path.col.loss"))}</th><th class="num">${esc(t("path.col.avg"))} (ms)</th><th class="num">${esc(t("path.col.worst"))}</th></tr></thead><tbody>${rows}</tbody></table>`;
    }
    out.push(`<section><h2>${esc(sectionTitle("path"))}</h2>${body}</section>`);
  }

  if (has("security")) {
    let body = `<p class="muted">${esc(t("reports.noData"))}</p>`;
    if (d.security) {
      const items = d.security.findings
        .map((f) => {
          const p = { ...fmtParams(d.lang, f.params), subject: f.subject ?? "" };
          return `<div class="finding" style="border-color:${LEVEL[f.severity]}"><b>[${esc(t(`severity.${f.severity}` as Key))}] ${esc(tm(`sec.${f.code}.title`, f.code, p))}</b><p>${esc(tm(`sec.${f.code}.fix`, "", p))}</p></div>`;
        })
        .join("");
      body = `<p class="big">${esc(t("security.score"))}: ${d.security.score}/100</p>${items || `<p>${esc(t("security.none"))}</p>`}`;
    }
    out.push(`<section><h2>${esc(sectionTitle("security"))}</h2>${body}</section>`);
  }

  if (has("monitor")) {
    let body = `<p class="muted">${esc(t("reports.noData"))}</p>`;
    const m = d.monitor;
    if (m && m.status.sampleCount > 0) {
      const chart = lineChart(
        [
          { values: m.series.gatewayMs, color: "#2433c9", label: t("monitor.series.gateway") },
          { values: m.series.internetMs, color: "#12805a", label: t("monitor.series.internet") },
        ],
        m.series.ts,
        d.lang,
        "ms",
      );
      const inc = m.status.incidents
        .filter((i) => i.kind !== "roam")
        .slice(0, 60)
        .map(
          (i) =>
            `<tr><td>${esc(fmtDateTime(d.lang, i.startMs))}</td><td>${esc(t(`incident.${i.kind}` as Key))}</td><td>${esc(t(`where.${INCIDENT_WHERE[i.kind]}` as Key))}</td><td class="num">${i.endMs ? esc(fmtDuration(i.endMs - i.startMs)) : esc(t("monitor.ongoing"))}</td></tr>`,
        )
        .join("");
      body = `<p>${esc(t("report.monitor.window", { from: fmtDateTime(d.lang, m.series.ts[0] ?? now), to: fmtDateTime(d.lang, m.series.ts[m.series.ts.length - 1] ?? now), n: m.status.sampleCount }))}</p>${chart}
        <h3>${esc(t("monitor.incidents"))}</h3>${inc ? `<table><thead><tr><th>${esc(t("monitor.col.when"))}</th><th>${esc(t("monitor.col.what"))}</th><th>${esc(t("monitor.col.where"))}</th><th class="num">${esc(t("monitor.col.duration"))}</th></tr></thead><tbody>${inc}</tbody></table>` : `<p>${esc(t("monitor.incidents.none"))}</p>`}`;
    }
    out.push(`<section><h2>${esc(sectionTitle("monitor"))}</h2>${body}</section>`);
  }

  return `<!doctype html><html lang="${d.lang}"><head><meta charset="utf-8"><title>${esc(t("report.cover.title"))}${d.client ? ` — ${esc(d.client)}` : ""}</title>
<style>
@page { size: A4; margin: 16mm 14mm; }
* { box-sizing: border-box; }
body { font-family: Barlow, "Segoe UI", Helvetica, Arial, sans-serif; color: #17202b; font-size: 11pt; line-height: 1.45; margin: 0; padding: 24px; max-width: 900px; margin: 0 auto; }
h1 { font-size: 30pt; margin: 8px 0 6px; letter-spacing: -0.01em; }
h2 { font-size: 16pt; border-bottom: 2px solid #17202b; padding-bottom: 4px; margin: 0 0 10px; }
h3 { font-size: 12pt; margin: 14px 0 6px; }
section { page-break-before: always; padding-top: 6px; }
section.cover { page-break-before: auto; min-height: 60vh; }
.brand { font-weight: 700; color: #2433c9; font-size: 13pt; }
.site { font-size: 13pt; margin: 4px 0; }
.meta { color: #4a5563; }
.verdict { display: flex; gap: 18px; align-items: center; margin: 28px 0; }
.grade { width: 96px; height: 96px; border-radius: 16px; color: #fff; font-size: 64pt; font-weight: 700; display: grid; place-items: center; line-height: 1; }
.verdict-text { font-size: 18pt; font-weight: 600; max-width: 30ch; }
.small { font-size: 9pt; color: #7d8794; }
.toc ol { margin: 6px 0 0; padding-left: 20px; }
.finding { border-left: 4px solid; padding: 6px 10px; margin: 0 0 8px; background: #f6f8fa; break-inside: avoid; }
.finding p { margin: 3px 0 0; color: #4a5563; }
table { width: 100%; border-collapse: collapse; font-size: 9pt; margin: 6px 0 10px; }
th, td { text-align: left; padding: 4px 6px; border-bottom: 1px solid #d5dbe2; vertical-align: top; }
th { background: #edf0f3; }
td.num, th.num { text-align: right; }
tr.hl td { background: #e5e8fb; }
.mono { letter-spacing: 0.02em; }
.muted { color: #7d8794; }
.big { font-size: 13pt; font-weight: 600; }
figure { margin: 0 0 14px; break-inside: avoid; }
figure img { width: 100%; border: 1px solid #d5dbe2; }
figcaption { font-size: 9pt; color: #4a5563; margin-top: 4px; }
.legend { display: flex; gap: 14px; font-size: 9pt; color: #4a5563; }
.legend i { display: inline-block; width: 10px; height: 10px; margin-right: 5px; vertical-align: -1px; }
footer { margin-top: 30px; font-size: 8pt; color: #7d8794; text-align: center; }
@media print { body { padding: 0; } }
</style></head><body>${out.join("\n")}<footer>${esc(t("report.footer"))}${company ? `, ${esc(company)}` : ""}</footer></body></html>`;
}
