import { useEffect, useState } from "react";
import { Check, CircleDashed, Loader2, Share2, Stethoscope } from "lucide-react";
import { call, saveDataUrl } from "../../api";
import { useApp, type ModuleId } from "../../store";
import { fmtNumber, translateMaybe, useLang, useT, type Key } from "../../i18n";
import type { Advice, Diagnosis, DiscoveryRun, PathRun, PerfRun } from "../../types";
import { fmtDateTime, fmtParams } from "../../lib/format";
import { quality, qualityColor } from "../../lib/signal";
import { Notice } from "../../components/ui";
import { ConsentDialog } from "../performance/PerformanceModule";
import { renderShareCard } from "./shareCard";

type Step = "wifi" | "performance" | "speed" | "path" | "discovery" | "security" | "advisor";
type StepState = "pending" | "running" | "done";

const GRADE_COLOR: Record<string, string> = {
  A: "var(--sig-excellent)",
  B: "var(--sig-good)",
  C: "var(--sig-fair)",
  D: "var(--sig-poor)",
  F: "var(--sig-bad)",
};

async function waitFor<T extends { running: boolean }>(prefix: string): Promise<T> {
  for (;;) {
    await new Promise((r) => setTimeout(r, 500));
    const s = await call<T>(`${prefix}.status`);
    if (!s.running) return s;
  }
}

let cachedDiagnosis: Diagnosis | null = null;

function AdviceItem({ a }: { a: Advice }) {
  const t = useT();
  const lang = useLang();
  const setModule = useApp((s) => s.setModule);
  const params = fmtParams(lang, a.params);
  const fix = translateMaybe(lang, `advice.${a.code}.fix`, "", params);
  return (
    <div className="finding" data-level={a.level}>
      <div className="finding-head">
        <span className="finding-title">{translateMaybe(lang, `advice.${a.code}.title`, a.code, params)}</span>
      </div>
      {fix && <p className="finding-body">{fix}</p>}
      {a.level !== "good" && (
        <button className="link-btn" onClick={() => setModule(a.module as ModuleId)}>
          {t("overview.evidence")}
        </button>
      )}
    </div>
  );
}

function StatCard({ label, value, sub, color, onClick, empty }: { label: string; value: string; sub?: string; color?: string; onClick?: () => void; empty?: boolean }) {
  return (
    <button className="stat-card" onClick={onClick} disabled={!onClick}>
      <span className="stat-label">{label}</span>
      <span className={empty ? "stat-empty" : "stat-value"} style={{ color }}>
        {value}
      </span>
      {sub && <span className="stat-sub">{sub}</span>}
    </button>
  );
}

export function OverviewModule() {
  const t = useT();
  const lang = useLang();
  const settings = useApp((s) => s.settings);
  const updateSettings = useApp((s) => s.updateSettings);
  const setModule = useApp((s) => s.setModule);
  const [diag, setDiag] = useState<Diagnosis | null>(cachedDiagnosis);
  const [steps, setSteps] = useState<Partial<Record<Step, StepState>> | null>(null);
  const [withSpeed, setWithSpeed] = useState(false);
  const [withDiscovery, setWithDiscovery] = useState(true);
  const [askConsent, setAskConsent] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    call<Diagnosis>("advisor.diagnose", { tzOffsetMin: -new Date().getTimezoneOffset() })
      .then((d) => {
        cachedDiagnosis = d;
        setDiag(d);
      })
      .catch((e) => setError(String(e.message ?? e)));
  }, []);
  useEffect(() => {
    if (settings?.mlabConsent) setWithSpeed(true);
  }, [settings?.mlabConsent]);

  const running = !!steps && Object.values(steps).some((s) => s !== "done");

  const checkup = async (speed: boolean) => {
    const plan: Step[] = ["wifi", "performance", ...(speed ? (["speed"] as Step[]) : []), "path", ...(withDiscovery ? (["discovery"] as Step[]) : []), "security", "advisor"];
    const state: Partial<Record<Step, StepState>> = Object.fromEntries(plan.map((s) => [s, "pending"]));
    const mark = (s: Step, v: StepState) => {
      state[s] = v;
      setSteps({ ...state });
    };
    setError(null);
    setSteps({ ...state });
    try {
      mark("wifi", "running");
      await call("wifi.scan", { trigger: true });
      mark("wifi", "done");
      mark("performance", "running");
      if (speed) mark("speed", "running");
      await call("perf.start", { latency: true, dns: true, speed });
      await waitFor<PerfRun>("perf");
      mark("performance", "done");
      if (speed) mark("speed", "done");
      mark("path", "running");
      await call("path.start", { target: "8.8.8.8", rounds: 10 });
      await waitFor<PathRun>("path");
      mark("path", "done");
      if (withDiscovery) {
        mark("discovery", "running");
        await call("discovery.start");
        await waitFor<DiscoveryRun>("discovery");
        mark("discovery", "done");
      }
      mark("security", "running");
      await call("security.scan");
      mark("security", "done");
      mark("advisor", "running");
      const d = await call<Diagnosis>("advisor.diagnose", { tzOffsetMin: -new Date().getTimezoneOffset() });
      cachedDiagnosis = d;
      setDiag(d);
      mark("advisor", "done");
    } catch (e) {
      setError(String((e as Error).message ?? e));
      setSteps(null);
    }
  };

  const start = () => {
    if (withSpeed && !settings?.mlabConsent) {
      setAskConsent(true);
      return;
    }
    checkup(withSpeed);
  };

  const share = async () => {
    if (!diag) return;
    const s = diag.summary;
    const stats = [
      s.downloadMbps != null ? { label: t("perf.speed.download"), value: `${fmtNumber(lang, s.downloadMbps)} Mb/s` } : null,
      s.internetLatencyMs != null ? { label: t("overview.card.latency"), value: `${fmtNumber(lang, s.internetLatencyMs)} ms` } : null,
      s.rssi != null ? { label: t("overview.card.wifi"), value: `${s.rssi} dBm` } : null,
      s.bufferbloat ? { label: t("perf.bloat.title"), value: s.bufferbloat } : null,
    ].filter(Boolean) as { label: string; value: string }[];
    const png = await renderShareCard(diag, {
      title: t("overview.share.title"),
      verdict: t(`verdict.${diag.verdict}` as Key),
      footer: `${t("overview.share.footer")}, ${fmtDateTime(lang, diag.generatedMs)}`,
      stats,
    });
    await saveDataUrl("whylag-result.png", png, [{ name: "PNG", extensions: ["png"] }]);
  };

  const s = diag?.summary;
  const problems = diag?.advice.filter((a) => a.level === "critical" || a.level === "warning") ?? [];
  const infos = diag?.advice.filter((a) => a.level === "info") ?? [];
  const goods = diag?.advice.filter((a) => a.level === "good") ?? [];
  const notChecked = !diag || diag.verdict === "notChecked" || diag.checked.length <= 1;
  const stepLabel: Record<Step, Key> = {
    wifi: "overview.step.wifi",
    performance: "overview.step.performance",
    speed: "overview.step.speed",
    path: "overview.step.path",
    discovery: "overview.step.discovery",
    security: "overview.step.security",
    advisor: "overview.step.advisor",
  };

  return (
    <div className="stack">
      {askConsent && (
        <ConsentDialog
          onAccept={async () => {
            await updateSettings({ mlabConsent: true });
            setAskConsent(false);
            checkup(true);
          }}
          onCancel={() => setAskConsent(false)}
        />
      )}
      {error && <Notice tone="crit">{error}</Notice>}
      <section className="hero">
        <div className="hero-grade" style={{ background: diag && !notChecked ? GRADE_COLOR[diag.grade] : "var(--rule-strong)" }} aria-label={t("overview.grade")}>
          {diag && !notChecked ? diag.grade : "?"}
        </div>
        <div className="hero-body">
          <div className="small muted">{t("overview.grade")}</div>
          <h2 className="hero-verdict">{t(`verdict.${notChecked ? "notChecked" : diag!.verdict}` as Key)}</h2>
          {notChecked ? (
            <p className="muted" style={{ margin: "0 0 14px" }}>
              {t("overview.notChecked")}
            </p>
          ) : (
            <p className="small muted" style={{ margin: "0 0 14px" }}>
              {t("overview.checked", { list: diag!.checked.map((c) => t(`nav.${c}` as Key)).join(", ") })}, {fmtDateTime(lang, diag!.generatedMs)}
            </p>
          )}
          {running && steps ? (
            <ol className="steps">
              {(Object.keys(steps) as Step[]).map((st) => (
                <li key={st} data-state={steps[st]}>
                  {steps[st] === "done" ? <Check size={15} /> : steps[st] === "running" ? <Loader2 size={15} className="spin" /> : <CircleDashed size={15} />}
                  {t(stepLabel[st])}
                </li>
              ))}
            </ol>
          ) : (
            <div className="row" style={{ gap: 14 }}>
              <button className="btn btn-primary" onClick={start}>
                <Stethoscope size={16} />
                {t("overview.checkup")}
              </button>
              <label className="check small">
                <input type="checkbox" checked={withSpeed} onChange={(e) => setWithSpeed(e.target.checked)} />
                {t("overview.checkup.withSpeed")}
              </label>
              <label className="check small">
                <input type="checkbox" checked={withDiscovery} onChange={(e) => setWithDiscovery(e.target.checked)} />
                {t("overview.checkup.withDiscovery")}
              </label>
              {!notChecked && (
                <button className="btn" onClick={share}>
                  <Share2 size={15} />
                  {t("overview.share")}
                </button>
              )}
            </div>
          )}
        </div>
      </section>

      {s && (
        <div className="stat-grid">
          <StatCard
            label={t("overview.card.wifi")}
            value={s.rssi != null ? `${s.rssi} dBm` : t("overview.card.notMeasured")}
            empty={s.rssi == null}
            sub={s.ssid ? `${s.ssid}${s.channel ? `, ch ${s.channel}` : ""}` : undefined}
            color={s.rssi != null ? qualityColor[quality(s.rssi)] : undefined}
            onClick={() => setModule("wifi")}
          />
          <StatCard
            label={t("overview.card.speed")}
            value={s.downloadMbps != null ? `${fmtNumber(lang, s.downloadMbps)} / ${fmtNumber(lang, s.uploadMbps ?? 0)}` : t("overview.card.notMeasured")}
            empty={s.downloadMbps == null}
            sub={s.downloadMbps != null ? `Mb/s${s.bufferbloat ? `, ${t("perf.bloat.grade", { grade: s.bufferbloat })}` : ""}` : undefined}
            onClick={() => setModule("performance")}
          />
          <StatCard
            label={t("overview.card.latency")}
            value={s.internetLatencyMs != null ? `${fmtNumber(lang, s.internetLatencyMs)} ms` : t("overview.card.notMeasured")}
            empty={s.internetLatencyMs == null}
            sub={s.gatewayLatencyMs != null ? `${t("perf.latency.gateway")}: ${fmtNumber(lang, s.gatewayLatencyMs, 1)} ms` : undefined}
            onClick={() => setModule("performance")}
          />
          <StatCard label={t("overview.card.devices")} value={s.devices != null ? String(s.devices) : t("overview.card.notMeasured")} empty={s.devices == null} onClick={() => setModule("discovery")} />
          <StatCard label={t("overview.card.security")} value={s.securityScore != null ? `${s.securityScore}/100` : t("overview.card.notMeasured")} empty={s.securityScore == null} onClick={() => setModule("security")} />
          <StatCard label={t("overview.card.incidents")} value={s.incidents24h != null ? String(s.incidents24h) : t("overview.card.notMeasured")} empty={s.incidents24h == null} onClick={() => setModule("monitor")} />
        </div>
      )}

      {!notChecked && (
        <div className="advice-columns">
          <section className="stack" style={{ gap: 8 }}>
            <h3 className="section-title">{t("overview.problems")}</h3>
            {problems.length === 0 ? <p className="muted small">{t("verdict.allGood")}</p> : problems.map((a, i) => <AdviceItem key={`${a.code}-${i}`} a={a} />)}
          </section>
          <section className="stack" style={{ gap: 8 }}>
            {infos.length > 0 && (
              <>
                <h3 className="section-title">{t("overview.info")}</h3>
                {infos.map((a, i) => (
                  <AdviceItem key={`${a.code}-${i}`} a={a} />
                ))}
              </>
            )}
            {goods.length > 0 && (
              <>
                <h3 className="section-title">{t("overview.goodNews")}</h3>
                {goods.map((a, i) => (
                  <AdviceItem key={`${a.code}-${i}`} a={a} />
                ))}
              </>
            )}
            <button className="link-btn" style={{ alignSelf: "flex-start" }} onClick={() => setModule("reports")}>
              {t("reports.technical")}
            </button>
          </section>
        </div>
      )}
    </div>
  );
}
