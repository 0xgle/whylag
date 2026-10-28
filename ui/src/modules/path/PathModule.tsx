import { useState } from "react";
import { Route, Square } from "lucide-react";
import { fmtNumber, translateMaybe, useLang, useT, type Key } from "../../i18n";
import type { PathFinding, PathRun } from "../../types";
import { useJob } from "../../lib/useJob";
import { Badge, Intro, Notice, Panel, Progress } from "../../components/ui";

const PRESETS = [
  { label: "Google DNS", host: "8.8.8.8" },
  { label: "Cloudflare", host: "1.1.1.1" },
  { label: "Microsoft Teams", host: "teams.microsoft.com" },
  { label: "YouTube", host: "www.youtube.com" },
];

const SEGMENT_COLOR: Record<string, string> = {
  lan: "#2433c9",
  isp: "#d9632b",
  internet: "#12805a",
  destination: "#17202b",
  unknown: "#7d8794",
};

function FindingNotice({ f }: { f: PathFinding }) {
  const t = useT();
  const lang = useLang();
  const tone = f.code === "healthy" || f.code === "rateLimited" || f.code === "cgnat" ? "info" : f.code === "lossFrom" ? "crit" : "warn";
  const text = translateMaybe(lang, `finding.${f.code}`, f.code, {
    ttl: f.ttl ?? "",
    value: f.value != null ? fmtNumber(lang, f.value) : "",
    segment: f.segment ? t(`segment.${f.segment}` as Key).toLowerCase() : "",
  });
  return <Notice tone={tone}>{text}</Notice>;
}

export function PathModule() {
  const t = useT();
  const lang = useLang();
  const { run, start, stop, error } = useJob<PathRun>("path", 350);
  const [target, setTarget] = useState(run?.target || "8.8.8.8");
  const running = !!run?.running;
  const go = (host = target) => {
    setTarget(host);
    start({ target: host.trim(), rounds: 20 });
  };
  const maxLatency = Math.max(20, ...(run?.hops ?? []).map((h) => h.worst ?? 0));

  const form = (
    <div className="toolbar">
      <input
        className="input"
        value={target}
        onChange={(e) => setTarget(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && !running && go()}
        aria-label={t("path.target")}
        placeholder={t("path.target")}
        style={{ width: 260 }}
      />
      {running ? (
        <button className="btn" onClick={stop}>
          <Square size={14} />
          {t("common.stop")}
        </button>
      ) : (
        <button className="btn btn-primary" onClick={() => go()} disabled={!target.trim()}>
          <Route size={15} />
          {t("path.run")}
        </button>
      )}
      <span className="chips">
        {PRESETS.map((p) => (
          <button key={p.host} className="chip" disabled={running} onClick={() => go(p.host)}>
            {p.label}
          </button>
        ))}
      </span>
      {running && run && <Progress value={run.progress} label={translateMaybe(lang, `path.phase.${run.phase}`, run.phase)} />}
    </div>
  );

  if (!run || (!running && run.hops.length === 0 && !run.error)) {
    return (
      <div className="stack">
        <Intro title={t("path.intro.title")} body={t("path.intro.body")} action={<div style={{ display: "inline-block" }}>{form}</div>} />
      </div>
    );
  }

  return (
    <div className="stack">
      {form}
      {error && <Notice tone="crit">{error}</Notice>}
      {run.error && <Notice tone="crit">{translateMaybe(lang, `path.error.${run.error}`, run.error)}</Notice>}
      {run.findings.length > 0 && (
        <div className="stack" style={{ gap: 8 }}>
          {run.findings.map((f, i) => (
            <FindingNotice key={i} f={f} />
          ))}
        </div>
      )}
      {run.hops.length > 0 && (
        <Panel
          title={`${run.target}${run.targetIp && run.targetIp !== run.target ? ` (${run.targetIp})` : ""}`}
          actions={<span className="small muted">{t("path.rounds", { n: run.rounds })}</span>}
          bodyClass=""
        >
          <div className="segment-legend">
            {(["lan", "isp", "internet", "destination"] as const).map((s) => (
              <span key={s}>
                <span className="swatch" style={{ background: SEGMENT_COLOR[s] }} />
                {t(`segment.${s}` as Key)}
              </span>
            ))}
          </div>
          <div className="table-wrap">
            <table className="data">
              <thead>
                <tr>
                  <th className="num">{t("path.col.hop")}</th>
                  <th>{t("path.col.host")}</th>
                  <th className="num">{t("path.col.loss")}</th>
                  <th className="num">{t("path.col.avg")}</th>
                  <th className="num">{t("path.col.best")}</th>
                  <th className="num">{t("path.col.worst")}</th>
                  <th style={{ width: "30%" }}>{t("path.col.latency")}</th>
                </tr>
              </thead>
              <tbody>
                {run.hops.map((h) => (
                  <tr key={h.ttl} style={{ cursor: "default" }}>
                    <td className="num">
                      <span className="hop-num" style={{ borderColor: SEGMENT_COLOR[h.segment] }}>
                        {h.ttl}
                      </span>
                    </td>
                    <td>
                      {h.ip ? (
                        <>
                          <span className="cell-strong">{h.name ?? h.ip}</span>
                          <span className="small muted" style={{ display: "block" }}>
                            {h.name ? `${h.ip}, ` : ""}
                            {t(`segment.${h.segment}` as Key)}
                          </span>
                        </>
                      ) : (
                        <span className="muted">{t("path.noReply")}</span>
                      )}
                    </td>
                    <td className="num">
                      {h.sent === 0 ? "—" : h.lossPct > 0 ? <Badge tone={h.lossPct >= 10 ? "crit" : "warn"}>{fmtNumber(lang, h.lossPct)}%</Badge> : "0%"}
                    </td>
                    <td className="num cell-strong">{h.avg != null ? fmtNumber(lang, h.avg, 1) : "—"}</td>
                    <td className="num">{h.best != null ? fmtNumber(lang, h.best, 1) : "—"}</td>
                    <td className="num">{h.worst != null ? fmtNumber(lang, h.worst, 1) : "—"}</td>
                    <td>
                      {h.avg != null && (
                        <span className="latency-bar" title={`${fmtNumber(lang, h.best ?? 0, 1)}–${fmtNumber(lang, h.worst ?? 0, 1)} ms`}>
                          <span
                            className="latency-range"
                            style={{
                              left: `${((h.best ?? 0) / maxLatency) * 100}%`,
                              width: `${Math.max(1, (((h.worst ?? 0) - (h.best ?? 0)) / maxLatency) * 100)}%`,
                              background: SEGMENT_COLOR[h.segment],
                            }}
                          />
                          <span className="latency-avg" style={{ left: `${(h.avg / maxLatency) * 100}%` }} />
                        </span>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Panel>
      )}
    </div>
  );
}
