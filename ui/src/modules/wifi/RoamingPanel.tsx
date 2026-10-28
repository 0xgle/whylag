import { useMemo } from "react";
import { Footprints, Square } from "lucide-react";
import { fmtNumber, useLang, useT } from "../../i18n";
import type { RoamEvent, RoamRun } from "../../types";
import { useJob } from "../../lib/useJob";
import { seriesColor } from "../../lib/signal";
import { Badge, Notice, Panel } from "../../components/ui";
import { TimeChart, type Series } from "../../components/TimeChart";

function short(bssid: string | null) {
  return bssid ? bssid.slice(-8) : "—";
}

function EventRow({ e }: { e: RoamEvent }) {
  const t = useT();
  const lang = useLang();
  const time = new Date(e.t).toLocaleTimeString(lang, { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  const side = (b: string | null, rssi: number | null, ch: number | null) => (
    <span>
      <span className="mono-ish">{short(b)}</span>
      {rssi != null && <span className="muted"> {rssi} dBm</span>}
      {ch != null && <span className="muted"> · ch {ch}</span>}
    </span>
  );
  return (
    <tr>
      <td className="num">{time}</td>
      <td>{t(`roam.kind.${e.kind}`)}</td>
      <td>{side(e.from, e.fromRssi, e.fromChannel)}</td>
      <td>{e.kind === "disconnect" ? "—" : side(e.to, e.toRssi, e.toChannel)}</td>
      <td className="num">{e.gapMs == null ? "…" : `${fmtNumber(lang, e.gapMs)} ms`}</td>
      <td>
        {e.sticky && <Badge tone="warn">{t("roam.sticky")}</Badge>} {e.slow && <Badge tone="crit">{t("roam.slow")}</Badge>}
        {!e.sticky && !e.slow && e.kind === "roam" && e.gapMs != null && <Badge tone="ok">{t("roam.smooth")}</Badge>}
      </td>
    </tr>
  );
}

/** Walk-around roaming test: which AP, how strong, and whether handovers stall. */
export function RoamingPanel() {
  const t = useT();
  const lang = useLang();
  const { run, start, stop } = useJob<RoamRun>("roam", 700);
  const running = !!run?.running;
  const events = run?.events ?? [];
  const roams = events.filter((e) => e.kind === "roam");
  const sticky = roams.filter((e) => e.sticky).length;
  const slow = events.filter((e) => e.slow).length;
  const drops = events.filter((e) => e.kind === "disconnect").length;
  const worst = Math.max(0, ...events.map((e) => e.gapMs ?? 0));

  const series: Series[] = useMemo(() => {
    const by = new Map<string, [number, number | null][]>();
    for (const s of run?.samples ?? []) {
      if (!s.bssid || s.rssi == null) continue;
      if (!by.has(s.bssid)) by.set(s.bssid, []);
      by.get(s.bssid)!.push([s.t, s.rssi]);
    }
    return [...by.entries()].map(([bssid, points]) => ({ label: short(bssid), color: seriesColor(bssid), points, width: 2 }));
  }, [run]);

  return (
    <Panel
      title={t("roam.title")}
      actions={
        running ? (
          <button className="btn" onClick={stop}>
            <Square size={14} />
            {t("roam.stop")}
          </button>
        ) : (
          <button className="btn btn-primary" onClick={() => start()}>
            <Footprints size={15} />
            {events.length ? t("roam.again") : t("roam.start")}
          </button>
        )
      }
    >
      <div className="stack" style={{ gap: 12 }}>
        <p className="small muted" style={{ margin: 0 }}>
          {running ? t("roam.walking", { ssid: run?.ssid ?? "Wi-Fi" }) : t("roam.body")}
        </p>
        {run && (run.samples.length > 0 || events.length > 0) && (
          <>
            <div className="roam-stats">
              <span>
                <b>{roams.length}</b> {t("roam.stat.roams")}
              </span>
              <span>
                <b>{sticky}</b> {t("roam.stat.sticky")}
              </span>
              <span>
                <b>{slow}</b> {t("roam.stat.slow")}
              </span>
              <span>
                <b>{drops}</b> {t("roam.stat.drops")}
              </span>
              <span>
                <b>{fmtNumber(lang, worst)}</b> ms {t("roam.stat.worst")}
              </span>
            </div>
            <TimeChart series={series} height={180} yRange={[-95, -30]} yLabel="dBm" ariaLabel={t("roam.title")} />
            {sticky > 0 && <Notice tone="warn">{t("roam.advice.sticky")}</Notice>}
            {slow > 0 && <Notice tone="warn">{t("roam.advice.slow")}</Notice>}
            {drops > 0 && <Notice tone="crit">{t("roam.advice.drops")}</Notice>}
            {!running && roams.length > 0 && sticky === 0 && slow === 0 && drops === 0 && <Notice tone="info">{t("roam.advice.good")}</Notice>}
            {events.length > 0 && (
              <div className="table-wrap">
                <table className="data">
                  <thead>
                    <tr>
                      <th className="num">{t("roam.col.time")}</th>
                      <th>{t("roam.col.event")}</th>
                      <th>{t("roam.col.from")}</th>
                      <th>{t("roam.col.to")}</th>
                      <th className="num">{t("roam.col.gap")}</th>
                      <th />
                    </tr>
                  </thead>
                  <tbody>
                    {[...events].reverse().map((e) => (
                      <EventRow key={e.t} e={e} />
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </>
        )}
      </div>
    </Panel>
  );
}
