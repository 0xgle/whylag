import { useEffect, useState } from "react";
import { ShieldCheck, ShieldAlert } from "lucide-react";
import { call } from "../../api";
import { translateMaybe, useLang, useT, type Key } from "../../i18n";
import type { SecurityFinding, SecurityReport, Severity } from "../../types";
import { fmtParams } from "../../lib/format";
import { Badge, Intro, Notice, Panel } from "../../components/ui";

const SEVERITY_TONE: Record<Severity, "crit" | "warn" | "info" | undefined> = {
  critical: "crit",
  high: "crit",
  medium: "warn",
  low: undefined,
  info: "info",
};

function scoreColor(score: number) {
  if (score >= 90) return "var(--sig-excellent)";
  if (score >= 75) return "var(--sig-good)";
  if (score >= 55) return "var(--sig-fair)";
  if (score >= 35) return "var(--sig-poor)";
  return "var(--sig-bad)";
}

export function FindingCard({ f }: { f: SecurityFinding }) {
  const t = useT();
  const lang = useLang();
  const params = { ...fmtParams(lang, f.params), subject: f.subject ?? "" };
  return (
    <div className="finding" data-level={f.severity}>
      <div className="finding-head">
        <Badge tone={SEVERITY_TONE[f.severity]}>{t(`severity.${f.severity}` as Key)}</Badge>
        <span className="finding-title">{translateMaybe(lang, `sec.${f.code}.title`, f.code, params)}</span>
      </div>
      <p className="finding-body">{translateMaybe(lang, `sec.${f.code}.fix`, "", params)}</p>
    </div>
  );
}

/** Ring gauge for a 0-100 score. */
export function ScoreRing({ score, size = 112 }: { score: number; size?: number }) {
  const r = size / 2 - 8;
  const c = 2 * Math.PI * r;
  return (
    <svg width={size} height={size} role="img" aria-label={`${score}/100`}>
      <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="var(--panel-2)" strokeWidth={10} />
      <circle
        cx={size / 2}
        cy={size / 2}
        r={r}
        fill="none"
        stroke={scoreColor(score)}
        strokeWidth={10}
        strokeDasharray={`${(score / 100) * c} ${c}`}
        strokeLinecap="round"
        transform={`rotate(-90 ${size / 2} ${size / 2})`}
      />
      <text x="50%" y="52%" textAnchor="middle" dominantBaseline="middle" className="ring-value">
        {score}
      </text>
    </svg>
  );
}

export function SecurityModule() {
  const t = useT();
  const [report, setReport] = useState<SecurityReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    call<SecurityReport | null>("security.last").then(setReport).catch(() => {});
  }, []);

  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      setReport(await call<SecurityReport>("security.scan"));
    } catch (e) {
      setError(String((e as Error).message ?? e));
    } finally {
      setBusy(false);
    }
  };

  if (!report) {
    return (
      <Intro
        title={t("security.intro.title")}
        body={t("security.intro.body")}
        action={
          <button className="btn btn-primary" onClick={run} disabled={busy}>
            <ShieldCheck size={16} />
            {t("security.run")}
          </button>
        }
      />
    );
  }

  const scope = [report.checkedWifi && t("security.scope.wifi"), report.checkedLan && t("security.scope.lan"), report.checkedDns && t("security.scope.dns")].filter(Boolean).join(", ");
  const groups: Severity[] = ["critical", "high", "medium", "low", "info"];

  return (
    <div className="stack">
      {error && <Notice tone="crit">{error}</Notice>}
      <Panel>
        <div className="row" style={{ gap: 24, flexWrap: "nowrap" }}>
          <ScoreRing score={report.score} />
          <div className="stack" style={{ gap: 6 }}>
            <div className="small muted">{t("security.score")}</div>
            <div className="headline">
              {report.findings.some((f) => f.severity === "critical" || f.severity === "high") ? (
                <ShieldAlert size={22} color="var(--sig-bad)" style={{ verticalAlign: -3, marginRight: 8 }} />
              ) : (
                <ShieldCheck size={22} color="var(--sig-good)" style={{ verticalAlign: -3, marginRight: 8 }} />
              )}
              {report.findings.filter((f) => f.severity !== "info").length === 0 ? t("security.none") : t("verdict.securityRisk")}
            </div>
            <div className="small muted">{t("security.scope", { list: scope || "—" })}</div>
            {(!report.checkedLan || !report.checkedDns) && <div className="small muted">{t("security.scope.missing")}</div>}
          </div>
          <span className="spacer" />
          <button className="btn" onClick={run} disabled={busy}>
            <ShieldCheck size={15} />
            {t("security.rerun")}
          </button>
        </div>
      </Panel>
      {groups.map((sev) => {
        const list = report.findings.filter((f) => f.severity === sev);
        if (!list.length) return null;
        return (
          <div key={sev} className="stack" style={{ gap: 8 }}>
            {list.map((f, i) => (
              <FindingCard key={`${f.code}-${i}`} f={f} />
            ))}
          </div>
        );
      })}
    </div>
  );
}
