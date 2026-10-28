import { useMemo, useState } from "react";
import { CheckCircle2, CircleSlash, ListChecks, Pencil, Play, Plus, Square, Trash2, TriangleAlert, XCircle } from "lucide-react";
import { useApp } from "../../store";
import { fmtNumber, translateMaybe, useLang, useT, type Key, type Lang } from "../../i18n";
import type { AutoTestCheck, AutoTestProfile, AutoTestRun, CheckStatus } from "../../types";
import { useJob } from "../../lib/useJob";
import { Field, Intro, Notice, Panel, Progress } from "../../components/ui";

const DEFAULT_PROFILE: AutoTestProfile = {
  name: "Standard",
  dhcpMs: 1000,
  gatewayMs: 20,
  gatewayLossPct: 1,
  dnsMs: 100,
  internetMs: 80,
  wifiRssiDbm: -67,
  wifiSnrDb: 25,
  wifiUtilPct: 60,
  linkMbps: 100,
  targets: [],
};

const ICON: Record<CheckStatus, typeof CheckCircle2> = { pass: CheckCircle2, warn: TriangleAlert, fail: XCircle, skip: CircleSlash };

/** Formats a check's parameters for its sentence: numbers localised, lists joined. */
function vars(lang: Lang, c: AutoTestCheck): Record<string, string | number> {
  const out: Record<string, string | number> = {};
  const num = (v: number) => fmtNumber(lang, v, v < 10 && !Number.isInteger(v) ? 1 : 0);
  for (const [k, v] of Object.entries(c.params)) {
    if (typeof v === "number") out[k] = num(v);
    else if (typeof v === "string") out[k] = v;
    else if (v == null) out[k] = "—";
  }
  if (c.value != null) out.value = num(c.value);
  if (c.limit != null) out.limit = num(c.limit);
  return out;
}

function CheckCard({ c }: { c: AutoTestCheck }) {
  const t = useT();
  const lang = useLang();
  const Icon = ICON[c.status];
  const v = vars(lang, c);
  const offers = c.id === "dhcp" && Array.isArray(c.params.offers) ? (c.params.offers as { server: string; offered: string; router: string | null; ms: number }[]) : [];
  return (
    <div className={`at-card at-${c.status}`}>
      <div className="at-head">
        <Icon size={20} aria-hidden />
        <span className="at-title">{c.id === "target" ? String(c.params.target ?? "") : t(`autotest.check.${c.id}` as Key)}</span>
        <span className="at-status">{t(`autotest.status.${c.status}` as Key)}</span>
      </div>
      {c.value != null && (
        <div className="at-value">
          <b>{v.value}</b> {c.unit}
          {c.limit != null && <span className="muted small"> · {t("autotest.limit", { limit: v.limit, unit: c.unit ?? "" })}</span>}
        </div>
      )}
      <p className="at-text">{translateMaybe(lang, `autotest.code.${c.code}`, c.code, v)}</p>
      {offers.length > 1 && (
        <ul className="at-offers">
          {offers.map((o) => (
            <li key={o.server}>{t("autotest.offer", { server: o.server, offered: o.offered, router: o.router ?? "—", ms: fmtNumber(lang, o.ms) })}</li>
          ))}
        </ul>
      )}
    </div>
  );
}

function ProfileEditor({ profile, onSave, onCancel, takenNames }: { profile: AutoTestProfile; onSave: (p: AutoTestProfile) => void; onCancel: () => void; takenNames: string[] }) {
  const t = useT();
  const [p, setP] = useState(profile);
  const [targets, setTargets] = useState(profile.targets.join("\n"));
  // Fields are edited as text ("-", "0." are valid on the way to a number)
  // and parsed when saving; an unparsable field keeps its previous value.
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const parsed = (): AutoTestProfile => {
    const out = { ...p };
    for (const [k, v] of Object.entries(drafts)) {
      const n = parseFloat(v.replace(",", "."));
      if (Number.isFinite(n)) (out as unknown as Record<string, number>)[k] = n;
    }
    return out;
  };
  const num = (key: keyof AutoTestProfile, label: Key, unit: string) => (
    <Field label={t(label)}>
      <span className="row" style={{ gap: 6 }}>
        <input
          className="input"
          style={{ width: 100 }}
          inputMode="decimal"
          value={drafts[key] ?? String(p[key])}
          onChange={(e) => setDrafts({ ...drafts, [key]: e.target.value })}
        />
        <span className="muted small">{unit}</span>
      </span>
    </Field>
  );
  const clash = p.name.trim() !== profile.name && takenNames.includes(p.name.trim());
  return (
    <Panel title={t("autotest.editProfile")}>
      <div className="stack">
        <Field label={t("autotest.field.name")}>
          <input className="input" style={{ width: 260 }} value={p.name} onChange={(e) => setP({ ...p, name: e.target.value })} />
        </Field>
        <div className="at-fields">
          {num("dhcpMs", "autotest.field.dhcpMs", "ms")}
          {num("gatewayMs", "autotest.field.gatewayMs", "ms")}
          {num("gatewayLossPct", "autotest.field.gatewayLossPct", "%")}
          {num("dnsMs", "autotest.field.dnsMs", "ms")}
          {num("internetMs", "autotest.field.internetMs", "ms")}
          {num("linkMbps", "autotest.field.linkMbps", "Mb/s")}
          {num("wifiRssiDbm", "autotest.field.wifiRssiDbm", "dBm")}
          {num("wifiSnrDb", "autotest.field.wifiSnrDb", "dB")}
          {num("wifiUtilPct", "autotest.field.wifiUtilPct", "%")}
        </div>
        <Field label={t("autotest.field.targets")} help={t("autotest.field.targetsHelp")}>
          <textarea className="input" rows={4} style={{ width: "100%", maxWidth: 420 }} value={targets} onChange={(e) => setTargets(e.target.value)} placeholder={"intranet.local:443\n192.168.1.10"} />
        </Field>
        {clash && <Notice tone="warn">{t("autotest.nameTaken")}</Notice>}
        <div className="row">
          <button
            className="btn btn-primary"
            disabled={!p.name.trim() || clash}
            onClick={() => onSave({ ...parsed(), name: p.name.trim(), targets: targets.split(/\n|,/).map((s) => s.trim()).filter(Boolean) })}
          >
            {t("autotest.save")}
          </button>
          <button className="btn btn-ghost" onClick={onCancel}>
            {t("common.cancel")}
          </button>
        </div>
      </div>
    </Panel>
  );
}

export function AutoTestModule() {
  const t = useT();
  const lang = useLang();
  const settings = useApp((s) => s.settings);
  const update = useApp((s) => s.updateSettings);
  const { run, start, stop, error } = useJob<AutoTestRun>("autotest", 300);
  const [editing, setEditing] = useState<AutoTestProfile | null>(null);

  const profiles = settings?.autotestProfiles?.length ? settings.autotestProfiles : [DEFAULT_PROFILE];
  const current = profiles.find((p) => p.name === settings?.autotestProfile) ?? profiles[0];
  const running = !!run?.running;
  const counts = useMemo(() => {
    const c = { pass: 0, warn: 0, fail: 0, skip: 0 };
    for (const ch of run?.checks ?? []) c[ch.status] += 1;
    return c;
  }, [run]);

  const saveProfile = async (p: AutoTestProfile) => {
    const original = editing?.name;
    const others = profiles.filter((x) => x.name !== original);
    await update({ autotestProfiles: [...others, p], autotestProfile: p.name });
    setEditing(null);
  };
  const removeProfile = async () => {
    const rest = profiles.filter((x) => x.name !== current.name);
    await update({ autotestProfiles: rest.length ? rest : [DEFAULT_PROFILE], autotestProfile: (rest[0] ?? DEFAULT_PROFILE).name });
  };

  const toolbar = (
    <div className="toolbar">
      <label className="row" style={{ gap: 8 }}>
        <span className="small muted">{t("autotest.profile")}</span>
        <select className="input" value={current.name} disabled={running} onChange={(e) => update({ autotestProfile: e.target.value })}>
          {profiles.map((p) => (
            <option key={p.name}>{p.name}</option>
          ))}
        </select>
      </label>
      <button className="btn btn-ghost" disabled={running} onClick={() => setEditing(current)} title={t("autotest.editProfile")}>
        <Pencil size={14} />
        {t("autotest.edit")}
      </button>
      <button className="btn btn-ghost" disabled={running} onClick={() => setEditing({ ...current, name: "" })}>
        <Plus size={14} />
        {t("autotest.newProfile")}
      </button>
      {profiles.length > 1 && (
        <button className="btn btn-ghost" disabled={running} onClick={removeProfile} title={t("autotest.deleteProfile")}>
          <Trash2 size={14} />
        </button>
      )}
      <span style={{ flex: 1 }} />
      {running ? (
        <button className="btn" onClick={stop}>
          <Square size={14} />
          {t("common.stop")}
        </button>
      ) : (
        <button className="btn btn-primary" onClick={() => start({ profile: current })}>
          <Play size={15} />
          {t("autotest.run")}
        </button>
      )}
    </div>
  );

  const editor = editing && <ProfileEditor profile={editing} takenNames={profiles.map((p) => p.name)} onSave={saveProfile} onCancel={() => setEditing(null)} />;

  if (!run || (!running && run.checks.length === 0 && !run.error)) {
    return (
      <div className="stack">
        <Intro
          title={t("autotest.intro.title")}
          body={t("autotest.intro.body")}
          action={
            <button className="btn btn-primary" onClick={() => start({ profile: current })}>
              <ListChecks size={16} />
              {t("autotest.run")}
            </button>
          }
        />
        {toolbar}
        {editor}
      </div>
    );
  }

  const verdict = counts.fail > 0 ? "fail" : counts.warn > 0 ? "warn" : "pass";
  return (
    <div className="stack">
      {toolbar}
      {editor}
      {running && <Progress value={run.progress} label={t("autotest.running")} />}
      {error && <Notice tone="crit">{error}</Notice>}
      {run.error && <Notice tone="crit">{translateMaybe(lang, `autotest.error.${run.error}`, run.error)}</Notice>}
      {!running && run.checks.length > 0 && (
        <div className={`at-summary at-${verdict}`}>
          <span className="at-summary-title">{t(`autotest.verdict.${verdict}` as Key)}</span>
          <span>{t("autotest.counts", { pass: counts.pass, warn: counts.warn, fail: counts.fail })}</span>
          {run.profile && <span className="muted small">{t("autotest.usedProfile", { name: run.profile.name })}</span>}
        </div>
      )}
      <div className="at-grid">
        {run.checks.map((c) => (
          <CheckCard key={`${c.id}-${c.order}`} c={c} />
        ))}
      </div>
    </div>
  );
}
