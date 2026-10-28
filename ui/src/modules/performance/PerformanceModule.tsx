import { useEffect, useState } from "react";
import { Gauge, Play, Square, Network } from "lucide-react";
import { call } from "../../api";
import { useApp } from "../../store";
import { fmtNumber, translateMaybe, useLang, useT, type Key } from "../../i18n";
import type { LatencyStats, PerfRun } from "../../types";
import { useJob } from "../../lib/useJob";
import { Badge, Intro, Notice, Panel, Progress, Sparkline } from "../../components/ui";
import { TimeChart } from "../../components/TimeChart";

const GRADE_TONE: Record<string, string> = {
  "A+": "var(--sig-excellent)",
  A: "var(--sig-good)",
  B: "var(--sig-fair)",
  C: "var(--sig-poor)",
  D: "var(--sig-bad)",
  F: "var(--sig-bad)",
};

function latencyColor(ms: number | null) {
  if (ms == null) return "var(--ink-3)";
  if (ms < 30) return "var(--sig-excellent)";
  if (ms < 60) return "var(--sig-good)";
  if (ms < 100) return "var(--sig-fair)";
  if (ms < 200) return "var(--sig-poor)";
  return "var(--sig-bad)";
}

export function ConsentDialog({ onAccept, onCancel }: { onAccept: () => void; onCancel: () => void }) {
  const t = useT();
  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true" aria-labelledby="consent-title">
      <div className="modal">
        <h2 id="consent-title" className="panel-title" style={{ fontSize: 18 }}>
          {t("perf.consent.title")}
        </h2>
        <p style={{ color: "var(--ink-2)" }}>{t("perf.consent.body")}</p>
        <div className="row" style={{ justifyContent: "flex-end" }}>
          <button className="btn btn-ghost" onClick={onCancel}>
            {t("common.cancel")}
          </button>
          <button className="btn btn-primary" onClick={onAccept}>
            {t("perf.consent.accept")}
          </button>
        </div>
      </div>
    </div>
  );
}

function SpeedPanel({ run }: { run: PerfRun }) {
  const t = useT();
  const lang = useLang();
  const settings = useApp((s) => s.settings);
  const sp = run.speed!;
  const series = [
    { label: t("perf.speed.download"), color: "#2433c9", points: sp.downSeries.map(([s, v]) => [Math.round(s * 100) / 100, v] as [number, number]) },
    { label: t("perf.speed.upload"), color: "#d9632b", points: sp.upSeries.map(([s, v]) => [Math.round((s + 11) * 100) / 100, v] as [number, number]) },
  ];
  const plan = (v: number | null | undefined, p: number | null | undefined) =>
    v != null && p ? <div className="small muted">{t("perf.speed.of", { plan: fmtNumber(lang, p) })} ({fmtNumber(lang, (v / p) * 100)}%)</div> : null;
  return (
    <div className="grid-2">
      <Panel title={t("perf.speed.title")}>
        <div className="speed-readouts">
          <div>
            <div className="small muted">{t("perf.speed.download")}</div>
            <div className="big-readout">
              {sp.downloadMbps != null ? fmtNumber(lang, sp.downloadMbps) : "—"}
              <span>Mb/s</span>
            </div>
            {plan(sp.downloadMbps, settings?.contractedDownMbps)}
          </div>
          <div>
            <div className="small muted">{t("perf.speed.upload")}</div>
            <div className="big-readout">
              {sp.uploadMbps != null ? fmtNumber(lang, sp.uploadMbps) : "—"}
              <span>Mb/s</span>
            </div>
            {plan(sp.uploadMbps, settings?.contractedUpMbps)}
          </div>
        </div>
        <TimeChart series={series} height={150} yLabel="Mb/s" xTime={false} ariaLabel={t("perf.speed.title")} />
        <div className="small muted">{t("perf.speed.server", { server: sp.server })}</div>
      </Panel>
      <Panel title={t("perf.bloat.title")} actions={sp.grade && <span className="grade" style={{ color: GRADE_TONE[sp.grade] }}>{t("perf.bloat.grade", { grade: sp.grade })}</span>}>
        <div className="bloat-bars">
          {(
            [
              ["perf.bloat.idle", sp.idleLatencyMs],
              ["perf.bloat.down", sp.loadedDownLatencyMs],
              ["perf.bloat.up", sp.loadedUpLatencyMs],
            ] as [Key, number | null][]
          ).map(([k, v]) => (
            <div key={k} className="bloat-row">
              <span className="bloat-label">{t(k)}</span>
              <span className="bloat-track">
                <span className="bloat-fill" style={{ width: `${Math.min(100, ((v ?? 0) / 400) * 100)}%`, background: latencyColor(v) }} />
              </span>
              <span className="bloat-val">{v != null ? `${fmtNumber(lang, v)} ms` : "—"}</span>
            </div>
          ))}
        </div>
        <p className="small muted" style={{ marginBottom: 0 }}>
          {t("perf.bloat.explain")}
        </p>
      </Panel>
    </div>
  );
}

function LatencyTable({ rows }: { rows: LatencyStats[] }) {
  const t = useT();
  const lang = useLang();
  const ms = (v: number | null) => (v != null ? `${fmtNumber(lang, v, v < 10 ? 1 : 0)} ms` : "—");
  return (
    <table className="data">
      <thead>
        <tr>
          <th>{t("perf.latency.target")}</th>
          <th className="num">{t("perf.latency.avg")}</th>
          <th className="num">{t("perf.latency.p95")}</th>
          <th className="num">{t("perf.latency.jitter")}</th>
          <th className="num">{t("perf.latency.loss")}</th>
          <th>{t("perf.latency.history")}</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((r) => {
          const vals = r.samples.filter((x): x is number => x != null);
          return (
            <tr key={r.id} style={{ cursor: "default" }}>
              <td>
                <span className="cell-strong">{r.id === "gateway" ? t("perf.latency.gateway") : r.label}</span>
                <span className="small muted" style={{ display: "block" }}>
                  {r.host}
                  {r.kind === "tcp" ? `, ${t("perf.latency.tcp")}` : ""}
                </span>
              </td>
              <td className="num cell-strong" style={{ color: latencyColor(r.avg) }}>
                {ms(r.avg)}
              </td>
              <td className="num">{ms(r.p95)}</td>
              <td className="num">{ms(r.jitter)}</td>
              <td className="num">{r.lossPct > 0 ? <Badge tone={r.lossPct >= 5 ? "crit" : "warn"}>{fmtNumber(lang, r.lossPct)}%</Badge> : "0%"}</td>
              <td>
                <Sparkline values={vals} min={0} max={Math.max(20, ...vals) * 1.15} width={120} height={26} color={latencyColor(r.avg)} />
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

function PeerPanel() {
  const t = useT();
  const lang = useLang();
  const [status, setStatus] = useState<{ listening: boolean; addresses: string[]; error: string | null } | null>(null);
  const [host, setHost] = useState("");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ downloadMbps: number; uploadMbps: number } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const toggle = async (enable: boolean) => setStatus(await call("perf.peerServer", { enable }));
  const test = async () => {
    setBusy(true);
    setErr(null);
    setResult(null);
    try {
      setResult(await call("perf.peerTest", { host: host.trim() }));
    } catch {
      setErr(t("perf.peer.unreachable"));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Panel title={t("perf.peer.title")}>
      <div className="stack" style={{ gap: 10 }}>
        <p className="small muted" style={{ margin: 0 }}>
          {t("perf.peer.body")}
        </p>
        <label className="check">
          <input type="checkbox" checked={!!status?.listening} onChange={(e) => toggle(e.target.checked)} />
          {t("perf.peer.accept")}
        </label>
        {status?.listening && <div className="small">{t("perf.peer.listening", { addresses: status.addresses.join(", ") })}</div>}
        {status?.error && <Notice tone="warn">{status.error}</Notice>}
        <div className="row">
          <input className="input" placeholder={t("perf.peer.host")} aria-label={t("perf.peer.host")} value={host} onChange={(e) => setHost(e.target.value)} style={{ width: 220 }} />
          <button className="btn" disabled={!host.trim() || busy} onClick={test}>
            <Network size={15} />
            {t("perf.peer.run")}
          </button>
          {busy && <Progress value={0.5} />}
        </div>
        {result && (
          <div className="cell-strong">
            {t("perf.peer.result", { down: fmtNumber(lang, result.downloadMbps), up: fmtNumber(lang, result.uploadMbps) })}
          </div>
        )}
        {err && <Notice tone="warn">{err}</Notice>}
      </div>
    </Panel>
  );
}

export function PerformanceModule() {
  const t = useT();
  const lang = useLang();
  const settings = useApp((s) => s.settings);
  const updateSettings = useApp((s) => s.updateSettings);
  const { run, start, stop, error } = useJob<PerfRun>("perf");
  const [withSpeed, setWithSpeed] = useState(true);
  const [askConsent, setAskConsent] = useState(false);

  useEffect(() => {
    if (settings && !settings.mlabConsent) setWithSpeed(false);
  }, [settings]);

  const begin = () => {
    if (withSpeed && !settings?.mlabConsent) {
      setAskConsent(true);
      return;
    }
    start({ latency: true, dns: true, speed: withSpeed });
  };
  const accept = async () => {
    await updateSettings({ mlabConsent: true });
    setAskConsent(false);
    start({ latency: true, dns: true, speed: true });
  };

  const running = !!run?.running;
  const controls = (
    <div className="toolbar">
      {running ? (
        <button className="btn" onClick={stop}>
          <Square size={14} />
          {t("common.stop")}
        </button>
      ) : (
        <button className="btn btn-primary" onClick={begin}>
          <Play size={15} />
          {t("perf.run")}
        </button>
      )}
      <label className="check">
        <input type="checkbox" checked={withSpeed} onChange={(e) => setWithSpeed(e.target.checked)} disabled={running} />
        {t("perf.includeSpeed")}
      </label>
      {running && run && <Progress value={run.progress} label={translateMaybe(lang, `perf.phase.${run.phase}`, t("perf.running"))} />}
    </div>
  );

  const fastest = run?.dns.filter((d) => d.medianMs != null).sort((a, b) => a.medianMs! - b.medianMs!)[0];
  const hasData = !!run && (run.latency.length > 0 || run.dns.length > 0 || !!run.speed);

  return (
    <div className="stack">
      {askConsent && <ConsentDialog onAccept={accept} onCancel={() => setAskConsent(false)} />}
      {!hasData && !running ? (
        <Intro
          title={t("perf.intro.title")}
          body={t("perf.intro.body")}
          action={
            <div className="stack" style={{ alignItems: "center", gap: 12 }}>
              <button className="btn btn-primary" onClick={begin}>
                <Gauge size={16} />
                {t("perf.run")}
              </button>
              <label className="check">
                <input type="checkbox" checked={withSpeed} onChange={(e) => setWithSpeed(e.target.checked)} />
                {t("perf.includeSpeed")}
              </label>
            </div>
          }
        />
      ) : (
        controls
      )}
      {error && <Notice tone="crit">{error}</Notice>}
      {run?.speedError === "consentRequired" && <Notice tone="warn">{t("perf.consent.required")}</Notice>}
      {run?.speedError && run.speedError !== "consentRequired" && run.speedError !== "cancelled" && (
        <Notice tone="warn">{t("perf.speed.failed", { detail: run.speedError })}</Notice>
      )}
      {run?.speed && <SpeedPanel run={run} />}
      {run && run.latency.length > 0 && (
        <Panel title={t("perf.latency.title")} bodyClass="">
          <div className="table-wrap">
            <LatencyTable rows={run.latency} />
          </div>
        </Panel>
      )}
      {run && run.dns.length > 0 && (
        <Panel title={t("perf.dns.title")} bodyClass="">
          <table className="data">
            <thead>
              <tr>
                <th>{t("perf.dns.server")}</th>
                <th className="num">{t("perf.dns.median")}</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {run.dns.map((d) => (
                <tr key={d.server} style={{ cursor: "default" }}>
                  <td>
                    <span className="cell-strong">{d.label}</span>
                    <span className="small muted" style={{ display: "block" }}>
                      {d.server}
                    </span>
                  </td>
                  <td className="num cell-strong" style={{ color: latencyColor(d.medianMs) }}>
                    {d.medianMs != null ? `${fmtNumber(lang, d.medianMs, 1)} ms` : "—"}
                  </td>
                  <td>
                    <span className="row" style={{ gap: 4 }}>
                      {d.system && <Badge tone="info">{t("perf.dns.yours")}</Badge>}
                      {fastest?.server === d.server && <Badge tone="ok">{t("perf.dns.fastest")}</Badge>}
                    </span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {run.dnsHijack != null && (
            <div className="panel-body">
              <Notice tone={run.dnsHijack ? "warn" : "info"}>{run.dnsHijack ? t("perf.dns.hijack") : t("perf.dns.noHijack")}</Notice>
            </div>
          )}
        </Panel>
      )}
      {(hasData || running) && <PeerPanel />}
    </div>
  );
}
