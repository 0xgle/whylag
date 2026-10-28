import { useCallback, useEffect, useMemo, useState } from "react";
import { Activity, Square, Trash2 } from "lucide-react";
import { call } from "../../api";
import { fmtNumber, useLang, useT, type Key } from "../../i18n";
import type { Incident, IncidentKind, MonitorStatus, SampleSeries } from "../../types";
import { fmtDuration, fmtTime } from "../../lib/format";
import { Badge, Intro, Panel, Segmented } from "../../components/ui";
import { TimeChart } from "../../components/TimeChart";

type Range = "1h" | "6h" | "24h" | "7d";
const RANGE_MS: Record<Range, number> = { "1h": 3_600_000, "6h": 6 * 3_600_000, "24h": 24 * 3_600_000, "7d": 7 * 24 * 3_600_000 };

export const INCIDENT_WHERE: Record<IncidentKind, "local" | "isp" | "dns" | "wifi"> = {
  localOutage: "local",
  ispOutage: "isp",
  localLoss: "local",
  ispLoss: "isp",
  latencySpike: "isp",
  localLatency: "local",
  dnsFailure: "dns",
  dnsSlow: "dns",
  weakSignal: "wifi",
  roam: "wifi",
};

const INCIDENT_TONE: Record<IncidentKind, "crit" | "warn" | "info" | undefined> = {
  localOutage: "crit",
  ispOutage: "crit",
  localLoss: "warn",
  ispLoss: "warn",
  latencySpike: "warn",
  localLatency: "warn",
  dnsFailure: "crit",
  dnsSlow: undefined,
  weakSignal: "warn",
  roam: "info",
};

function peakText(lang: "en" | "pl", i: Incident) {
  if (i.peak == null || i.peak === 0) return "—";
  switch (i.kind) {
    case "localLoss":
    case "ispLoss":
      return `${fmtNumber(lang, i.peak)}%`;
    case "weakSignal":
    case "roam":
      return `${fmtNumber(lang, i.peak)} dBm`;
    default:
      return `${fmtNumber(lang, i.peak)} ms`;
  }
}

export function MonitorModule() {
  const t = useT();
  const lang = useLang();
  const [status, setStatus] = useState<MonitorStatus | null>(null);
  const [series, setSeries] = useState<SampleSeries | null>(null);
  const [range, setRange] = useState<Range>("24h");
  const [interval, setIntervalS] = useState(5);

  const refresh = useCallback(async () => {
    const s = await call<MonitorStatus>("monitor.status");
    setStatus(s);
    const to = Date.now();
    setSeries(await call<SampleSeries>("monitor.series", { fromMs: to - RANGE_MS[range], toMs: to, points: 720 }));
  }, [range]);

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, 4000);
    return () => clearInterval(id);
  }, [refresh]);

  const start = async () => {
    setStatus(await call<MonitorStatus>("monitor.start", { intervalS: interval }));
    refresh();
  };
  const stop = async () => setStatus(await call<MonitorStatus>("monitor.stop"));
  const clear = async () => {
    if (!window.confirm(t("monitor.clear.confirm"))) return;
    await call("monitor.clear");
    refresh();
  };

  const charts = useMemo(() => {
    if (!series) return null;
    const pts = (vals: (number | null)[]) => series.ts.map((ts, i) => [ts, vals[i]] as [number, number | null]);
    return {
      latency: [
        { label: t("monitor.series.gateway"), color: "#2433c9", points: pts(series.gatewayMs) },
        { label: t("monitor.series.internet"), color: "#12805a", points: pts(series.internetMs), width: 2 },
        { label: t("monitor.series.dns"), color: "#c99a12", points: pts(series.dnsMs), width: 1.2 },
      ],
      loss: [
        { label: t("monitor.series.inetLoss"), color: "#c0263d", points: pts(series.internetLoss.map((v) => v * 100)) },
        { label: t("monitor.series.gwLoss"), color: "#d9632b", points: pts(series.gatewayLoss.map((v) => v * 100)) },
      ],
      signal: [{ label: "RSSI", color: "#2433c9", points: pts(series.rssi) }],
    };
  }, [series, t]);

  if (!status) return <div className="empty">{t("common.loading")}</div>;
  const from = Date.now() - RANGE_MS[range];
  const incidents = status.incidents.filter((i) => i.startMs >= from);

  if (!status.running && status.sampleCount === 0) {
    return (
      <Intro
        title={t("monitor.intro.title")}
        body={t("monitor.intro.body")}
        action={
          <button className="btn btn-primary" onClick={start}>
            <Activity size={16} />
            {t("monitor.start")}
          </button>
        }
      />
    );
  }

  return (
    <div className="stack">
      <div className="toolbar">
        {status.running ? (
          <button className="btn" onClick={stop}>
            <Square size={14} />
            {t("monitor.stop")}
          </button>
        ) : (
          <button className="btn btn-primary" onClick={start}>
            <Activity size={15} />
            {t("monitor.start")}
          </button>
        )}
        {!status.running && (
          <label className="row" style={{ gap: 6 }}>
            <span className="small muted">{t("monitor.interval")}</span>
            <select className="input" value={interval} onChange={(e) => setIntervalS(Number(e.target.value))} style={{ width: 80 }}>
              {[2, 5, 10, 30, 60].map((s) => (
                <option key={s} value={s}>
                  {s} s
                </option>
              ))}
            </select>
          </label>
        )}
        <span className="small muted">
          {status.running && status.startedMs ? (
            <>
              <span className="dot pulse-dot" style={{ display: "inline-block", color: "var(--sig-good)", marginRight: 6 }} />
              {t("monitor.running", { s: status.intervalS, time: fmtTime(lang, status.startedMs) })}
            </>
          ) : (
            t("monitor.stopped")
          )}
          {", "}
          {t("monitor.samples", { n: fmtNumber(lang, status.sampleCount) })}
        </span>
        <span className="spacer" />
        <Segmented<Range>
          value={range}
          onChange={setRange}
          options={(["1h", "6h", "24h", "7d"] as Range[]).map((r) => ({ value: r, label: t(`monitor.range.${r}` as Key) }))}
        />
        <button className="btn btn-ghost btn-sm" onClick={clear} title={t("monitor.clear")} aria-label={t("monitor.clear")}>
          <Trash2 size={15} />
        </button>
      </div>

      {charts && (
        <>
          <Panel title={t("monitor.chart.latency")}>
            <TimeChart series={charts.latency} height={220} yLabel="ms" ariaLabel={t("monitor.chart.latency")} />
          </Panel>
          <div className="grid-2">
            <Panel title={t("monitor.chart.loss")}>
              <TimeChart series={charts.loss} height={150} yRange={[0, 100]} yLabel="%" ariaLabel={t("monitor.chart.loss")} />
            </Panel>
            <Panel title={t("monitor.chart.signal")}>
              <TimeChart series={charts.signal} height={150} yRange={[-90, -30]} yLabel="dBm" ariaLabel={t("monitor.chart.signal")} />
            </Panel>
          </div>
        </>
      )}

      <Panel title={`${t("monitor.incidents")} (${incidents.length})`} bodyClass="">
        {incidents.length === 0 ? (
          <div className="empty">{t("monitor.incidents.none")}</div>
        ) : (
          <div className="table-wrap" style={{ maxHeight: 420 }}>
            <table className="data">
              <thead>
                <tr>
                  <th>{t("monitor.col.when")}</th>
                  <th>{t("monitor.col.what")}</th>
                  <th>{t("monitor.col.where")}</th>
                  <th className="num">{t("monitor.col.duration")}</th>
                  <th className="num">{t("monitor.col.peak")}</th>
                </tr>
              </thead>
              <tbody>
                {incidents.map((i) => (
                  <tr key={i.id} style={{ cursor: "default" }}>
                    <td>{fmtTime(lang, i.startMs, true)}</td>
                    <td>
                      <Badge tone={INCIDENT_TONE[i.kind]}>{t(`incident.${i.kind}` as Key)}</Badge>
                      {i.detail && <span className="small muted"> {i.detail}</span>}
                    </td>
                    <td>{t(`where.${INCIDENT_WHERE[i.kind]}` as Key)}</td>
                    <td className="num">{i.endMs ? fmtDuration(i.endMs - i.startMs) : t("monitor.ongoing")}</td>
                    <td className="num">{peakText(lang, i)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Panel>
    </div>
  );
}
