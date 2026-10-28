import { useEffect, useMemo, useRef, useState } from "react";
import { Copy, FileText, Printer, Save, X } from "lucide-react";
import { call, saveTextFile } from "../../api";
import { useApp } from "../../store";
import { useLang, useT, type Key, type Lang } from "../../i18n";
import type { Diagnosis, DiscoveryRun, Heatmap, LocatedAp, MonitorStatus, PathRun, PerfRun, SampleSeries, ScanResult, SecurityReport } from "../../types";
import { Notice, Panel, Segmented, Toast, useToast } from "../../components/ui";
import { allFloors, useSurvey } from "../survey/surveyStore";
import { renderSurveyPng } from "../survey/render";
import { buildReport, type SectionId } from "./buildReport";
import { buildComplaint, hasEvidence } from "./complaint";

const SECTIONS: SectionId[] = ["summary", "wifi", "survey", "discovery", "performance", "path", "security", "monitor"];

function Preview({ html, onClose, title }: { html: string; onClose: () => void; title: string }) {
  const t = useT();
  const frame = useRef<HTMLIFrameElement>(null);
  return (
    <div className="preview" role="dialog" aria-modal="true" aria-label={title}>
      <div className="preview-bar">
        <span className="cell-strong">{title}</span>
        <span className="spacer" />
        <button className="btn btn-primary" onClick={() => frame.current?.contentWindow?.print()}>
          <Printer size={15} />
          {t("reports.print")}
        </button>
        <button className="btn" onClick={() => saveTextFile(`${title.replace(/[^\w\- ]+/g, "").trim() || "report"}.html`, html, [{ name: "HTML", extensions: ["html"] }])}>
          <Save size={15} />
          {t("reports.saveHtml")}
        </button>
        <button className="btn btn-ghost" onClick={onClose} aria-label={t("common.close")}>
          <X size={16} />
        </button>
      </div>
      <iframe ref={frame} title={title} srcDoc={html} className="preview-frame" />
    </div>
  );
}

export function ReportsModule() {
  const t = useT();
  const lang = useLang();
  const settings = useApp((s) => s.settings);
  const project = useSurvey((s) => s.project);
  const threshold = settings?.coverageThresholdDbm ?? -67;
  const [client, setClient] = useState("");
  const [sections, setSections] = useState<SectionId[]>(() => {
    const preset = useApp.getState().reportPreset as SectionId[] | null;
    return preset?.length ? SECTIONS.filter((s) => preset.includes(s)) : SECTIONS;
  });
  // The preset applies once, to this visit.
  useEffect(() => {
    useApp.getState().setReportPreset(null);
  }, []);
  const [busy, setBusy] = useState(false);
  const [html, setHtml] = useState<string | null>(null);
  const [toast, showToast] = useToast();

  // Complaint state
  const [name, setName] = useState("");
  const [address, setAddress] = useState("");
  const [customer, setCustomer] = useState("");
  const [isp, setIsp] = useState("");
  const [letterLang, setLetterLang] = useState<Lang>(lang);
  const [ev, setEv] = useState<{ perf: PerfRun | null; path: PathRun | null; monitor: MonitorStatus | null } | null>(null);

  useEffect(() => {
    Promise.all([call<PerfRun>("perf.status"), call<PathRun>("path.status"), call<MonitorStatus>("monitor.status")])
      .then(([perf, path, monitor]) => setEv({ perf, path, monitor }))
      .catch(() => {});
  }, []);

  const complaintInput = { lang: letterLang, name, address, customer, isp, settings, perf: ev?.perf ?? null, path: ev?.path ?? null, monitor: ev?.monitor ?? null };
  const letter = useMemo(() => buildComplaint(complaintInput), [letterLang, name, address, customer, isp, settings, ev]); // eslint-disable-line react-hooks/exhaustive-deps
  const evidence = hasEvidence(complaintInput);

  const generate = async () => {
    setBusy(true);
    try {
      const [diag, scan, discovery, perf, path, security, status] = await Promise.all([
        call<Diagnosis>("advisor.diagnose", { tzOffsetMin: -new Date().getTimezoneOffset() }).catch(() => null),
        call<ScanResult>("wifi.scan").catch(() => null),
        call<DiscoveryRun>("discovery.status").catch(() => null),
        call<PerfRun>("perf.status").catch(() => null),
        call<PathRun>("path.status").catch(() => null),
        call<SecurityReport | null>("security.last").catch(() => null),
        call<MonitorStatus>("monitor.status").catch(() => null),
      ]);
      const to = Date.now();
      const series = status && status.sampleCount ? await call<SampleSeries>("monitor.series", { fromMs: to - 24 * 3_600_000, toMs: to, points: 400 }) : null;
      let survey = null;
      if (project && sections.includes("survey")) {
        const floors = [];
        // Every floor of the building, each with its own maps and located APs.
        for (const floor of allFloors(project)) {
          const mode: "measured" | "predicted" | null = floor.samples.length ? "measured" : floor.aps.length ? "predicted" : null;
          if (!mode) continue;
          const images = [];
          let areaM2: number | null = null;
          for (const layer of ["signal", "secondSignal", "snr", "coChannel", "bestAp"] as const) {
            const h = await call<Heatmap>("survey.heatmap", {
              mode,
              layer,
              widthPx: floor.floorPlan.widthPx,
              heightPx: floor.floorPlan.heightPx,
              metersPerPx: floor.floorPlan.metersPerPx,
              samples: mode === "measured" ? floor.samples : [],
              walls: floor.walls,
              aps: mode === "predicted" ? floor.aps : [],
              filter: { ssids: useSurvey.getState().ssids, bands: useSurvey.getState().bands },
              thresholdDbm: threshold,
              reachM: 5,
            });
            areaM2 ??= h.stats.areaM2;
            images.push({ layer, png: await renderSurveyPng(floor, h, { maxWidth: 1400 }), coverage: h.stats.cells ? h.stats.coverageRatio : null });
          }
          const aps =
            mode === "measured" && floor.samples.length >= 4
              ? await call<LocatedAp[]>("survey.locateAps", { samples: floor.samples, metersPerPx: floor.floorPlan.metersPerPx }).catch(() => [])
              : [];
          floors.push({ name: floor.floorName ?? floor.floorPlan.name, mode, images, aps, samples: floor.samples.length, areaM2 });
        }
        if (floors.length) survey = { name: project.name, floors };
      }
      setHtml(
        buildReport({
          lang,
          client,
          sections,
          settings,
          diag,
          scan,
          survey,
          discovery: discovery && discovery.hosts.length ? discovery : null,
          perf,
          path,
          security,
          monitor: status && series ? { status, series } : null,
        }),
      );
    } finally {
      setBusy(false);
    }
  };

  const copyLetter = async () => {
    try {
      await navigator.clipboard.writeText(letter);
      showToast(t("common.copied"));
    } catch {
      /* clipboard unavailable */
    }
  };

  return (
    <div className="stack" style={{ maxWidth: 1100 }}>
      <p className="muted" style={{ margin: 0 }}>
        {t("reports.intro")}
      </p>
      <div className="grid-2" style={{ alignItems: "start" }}>
        <Panel title={t("reports.technical")}>
          <div className="stack" style={{ gap: 12 }}>
            <p className="small muted" style={{ margin: 0 }}>
              {t("reports.technical.body")}
            </p>
            <label className="side-field">
              <span>{t("reports.client")}</span>
              <input className="input" value={client} onChange={(e) => setClient(e.target.value)} />
            </label>
            <div className="side-field">
              <span>{t("reports.sections")}</span>
              <div className="check-grid">
                {SECTIONS.map((s) => (
                  <label key={s} className="check">
                    <input
                      type="checkbox"
                      checked={sections.includes(s)}
                      onChange={(e) => setSections((cur) => (e.target.checked ? SECTIONS.filter((x) => cur.includes(x) || x === s) : cur.filter((x) => x !== s)))}
                    />
                    {t(`reports.section.${s}` as Key)}
                  </label>
                ))}
              </div>
            </div>
            <button className="btn btn-primary" onClick={generate} disabled={busy || !sections.length} style={{ alignSelf: "flex-start" }}>
              <FileText size={15} />
              {busy ? t("common.loading") : t("reports.generate")}
            </button>
          </div>
        </Panel>

        <Panel title={t("reports.complaint")}>
          <div className="stack" style={{ gap: 10 }}>
            <p className="small muted" style={{ margin: 0 }}>
              {t("reports.complaint.body")}
            </p>
            <div className="form-grid">
              <label className="side-field">
                <span>{t("complaint.name")}</span>
                <input className="input" value={name} onChange={(e) => setName(e.target.value)} />
              </label>
              <label className="side-field">
                <span>{t("complaint.isp")}</span>
                <input className="input" value={isp} onChange={(e) => setIsp(e.target.value)} />
              </label>
              <label className="side-field">
                <span>{t("complaint.address")}</span>
                <input className="input" value={address} onChange={(e) => setAddress(e.target.value)} />
              </label>
              <label className="side-field">
                <span>{t("complaint.customer")}</span>
                <input className="input" value={customer} onChange={(e) => setCustomer(e.target.value)} />
              </label>
            </div>
            <div className="row">
              <span className="small muted">{t("complaint.language")}</span>
              <Segmented<Lang>
                value={letterLang}
                onChange={setLetterLang}
                options={[
                  { value: "pl", label: "Polski" },
                  { value: "en", label: "English" },
                ]}
              />
            </div>
            {!evidence && <Notice tone="warn">{t("complaint.missing")}</Notice>}
            <textarea className="letter" readOnly value={letter} aria-label={t("complaint.letter")} />
            <div className="row">
              <button className="btn" onClick={copyLetter}>
                <Copy size={15} />
                {t("complaint.copy")}
              </button>
              <button className="btn" onClick={() => saveTextFile(`${letterLang === "pl" ? "reklamacja" : "complaint"}.txt`, letter, [{ name: "Text", extensions: ["txt"] }])}>
                <Save size={15} />
                {t("complaint.save")}
              </button>
            </div>
            <p className="small muted" style={{ margin: 0 }}>
              {t("complaint.note")}
            </p>
          </div>
        </Panel>
      </div>
      {html && <Preview html={html} title={`${t("report.cover.title")}${client ? ` ${client}` : ""}`} onClose={() => setHtml(null)} />}
      <Toast message={toast} />
    </div>
  );
}
