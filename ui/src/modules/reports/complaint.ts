import { fmtNumber, type Lang } from "../../i18n";
import type { MonitorStatus, PathRun, PerfRun, Settings } from "../../types";
import { fmtDateTime, fmtDuration } from "../../lib/format";

export interface ComplaintInput {
  lang: Lang;
  name: string;
  address: string;
  customer: string;
  isp: string;
  settings: Settings | null;
  perf: PerfRun | null;
  path: PathRun | null;
  monitor: MonitorStatus | null;
}

/** Evidence the letter can cite. Returns null when there is nothing measured. */
function evidence(c: ComplaintInput) {
  const sp = c.perf?.speed;
  const outages = (c.monitor?.incidents ?? []).filter((i) => i.kind === "ispOutage" || i.kind === "ispLoss").slice(0, 25);
  const spikes = (c.monitor?.incidents ?? []).filter((i) => i.kind === "latencySpike").length;
  const ispLoss = c.path?.findings.find((f) => f.code === "lossFrom" && f.segment === "isp");
  if (!sp && !outages.length && !spikes && !ispLoss) return null;
  return { sp, outages, spikes, ispLoss };
}

export function hasEvidence(c: ComplaintInput) {
  return evidence(c) != null;
}

/** Builds the complaint letter in Polish or English from the measurements. */
export function buildComplaint(c: ComplaintInput): string {
  const ev = evidence(c);
  const n = (v: number | null | undefined) => (v == null ? "—" : fmtNumber(c.lang, v));
  const today = fmtDateTime(c.lang, Date.now()).split(",")[0];
  const down = c.settings?.contractedDownMbps;
  const up = c.settings?.contractedUpMbps;
  const sp = ev?.sp;
  const outageLines = (ev?.outages ?? []).map(
    (i) => `- ${fmtDateTime(c.lang, i.startMs)}${i.endMs ? ` (${fmtDuration(i.endMs - i.startMs)})` : ""}`,
  );

  if (c.lang === "pl") {
    const lines: string[] = [
      `${today}`,
      "",
      c.name || "[Imię i nazwisko]",
      c.address || "[Adres instalacji]",
      `Numer klienta/umowy: ${c.customer || "[numer]"}`,
      "",
      c.isp || "[Operator]",
      "Dział reklamacji",
      "",
      "Reklamacja jakości usługi dostępu do internetu",
      "",
      "Zgłaszam reklamację dotyczącą nienależytego wykonania usługi dostępu do internetu świadczonej pod powyższym adresem. Usługa nie osiąga parametrów, za które płacę, co potwierdzają poniższe pomiary.",
      "",
    ];
    if (sp) {
      lines.push(
        "1. Prędkość łącza",
        `Zmierzona prędkość pobierania: ${n(sp.downloadMbps)} Mb/s${down ? ` (umowa: ${n(down)} Mb/s, ${n(((sp.downloadMbps ?? 0) / down) * 100)}%)` : ""}.`,
        `Zmierzona prędkość wysyłania: ${n(sp.uploadMbps)} Mb/s${up ? ` (umowa: ${n(up)} Mb/s)` : ""}.`,
        `Pomiar wykonano narzędziem WhyLag z serwerem testowym M-Lab (${sp.server}).`,
        `Opóźnienie pod obciążeniem wzrosło z ${n(sp.idleLatencyMs)} ms do ${n(Math.max(sp.loadedDownLatencyMs ?? 0, sp.loadedUpLatencyMs ?? 0))} ms.`,
        "",
      );
    }
    if (ev?.outages.length) {
      lines.push(
        `${sp ? "2" : "1"}. Przerwy w działaniu usługi`,
        "Ciągły monitoring zarejestrował przerwy, podczas których mój router był dostępny, a brak połączenia występował poza moją siecią, po stronie operatora:",
        ...outageLines,
        "",
      );
    }
    if (ev?.spikes) {
      lines.push(`Zarejestrowano także skoki opóźnienia przy prawidłowo działającym routerze (liczba: ${ev.spikes}).`, "");
    }
    if (ev?.ispLoss) {
      lines.push(`Analiza trasy wykazała utratę ${n(ev.ispLoss.value)}% pakietów, zaczynającą się w sieci operatora (węzeł ${ev.ispLoss.ttl}).`, "");
    }
    lines.push(
      "W związku z powyższym wnoszę o:",
      "- usunięcie przyczyny nieprawidłowego działania usługi,",
      "- obniżenie opłaty abonamentowej za okres nienależytego świadczenia usługi,",
      "- pisemną odpowiedź na niniejszą reklamację.",
      "",
      "Dane pomiarowe mogę przekazać w formie raportu.",
      "",
      "Z poważaniem,",
      c.name || "[podpis]",
    );
    return lines.join("\n");
  }

  const lines: string[] = [
    `${today}`,
    "",
    c.name || "[Your name]",
    c.address || "[Service address]",
    `Customer/contract number: ${c.customer || "[number]"}`,
    "",
    c.isp || "[Internet provider]",
    "Customer complaints",
    "",
    "Complaint about internet service quality",
    "",
    "I am writing to complain that the internet service at the address above does not perform as contracted. The measurements below document the problem.",
    "",
  ];
  if (sp) {
    lines.push(
      "1. Connection speed",
      `Measured download speed: ${n(sp.downloadMbps)} Mb/s${down ? ` (contract: ${n(down)} Mb/s, ${n(((sp.downloadMbps ?? 0) / down) * 100)}%)` : ""}.`,
      `Measured upload speed: ${n(sp.uploadMbps)} Mb/s${up ? ` (contract: ${n(up)} Mb/s)` : ""}.`,
      `Measured with WhyLag against the M-Lab test server ${sp.server}.`,
      `Latency under load rose from ${n(sp.idleLatencyMs)} ms to ${n(Math.max(sp.loadedDownLatencyMs ?? 0, sp.loadedUpLatencyMs ?? 0))} ms.`,
      "",
    );
  }
  if (ev?.outages.length) {
    lines.push(
      `${sp ? "2" : "1"}. Service interruptions`,
      "Continuous monitoring recorded the following interruptions. During each of them my router remained reachable, so the fault was outside my premises, on the provider's side:",
      ...outageLines,
      "",
    );
  }
  if (ev?.spikes) {
    lines.push(`Monitoring also recorded ${ev.spikes} latency spikes while my router responded normally.`, "");
  }
  if (ev?.ispLoss) {
    lines.push(`A route analysis showed ${n(ev.ispLoss.value)}% packet loss starting inside the provider's network (hop ${ev.ispLoss.ttl}).`, "");
  }
  lines.push(
    "I therefore request that you:",
    "- fix the cause of the faulty service,",
    "- reduce my fee for the period the service was not delivered as contracted,",
    "- reply to this complaint in writing.",
    "",
    "I can provide the full measurement report on request.",
    "",
    "Yours sincerely,",
    c.name || "[signature]",
  );
  return lines.join("\n");
}
