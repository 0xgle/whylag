import { useMemo, useState, type ReactNode } from "react";
import {
  FileText,
  BrickWall,
  Crosshair,
  Download,
  Footprints,
  Hand,
  MapPinPlus,
  Redo2,
  Ruler,
  Save,
  Trash2,
  Undo2,
  Wand2,
  X,
} from "lucide-react";
import { call, saveDataUrl, saveTextFile } from "../../api";
import { useApp } from "../../store";
import { fmtNumber, useLang, useT, type Key } from "../../i18n";
import type { Band, ChannelPlan, Heatmap, Layer, Material, PlacementSuggestion, VirtualAp } from "../../types";
import { Segmented } from "../../components/ui";
import { legendStops, unitFor, colorFor, cssColor } from "./colors";
import { useSurvey, type Tool } from "./surveyStore";
import { renderSurveyPng } from "./render";

const LAYERS: Layer[] = ["signal", "secondSignal", "snr", "apCount", "coChannel", "bestAp", "throughput"];
const MATERIALS: Material[] = ["drywall", "glass", "wood", "brick", "concrete", "metal", "shaft"];
const MATERIAL_LOSS: Record<Material, [number, number]> = {
  drywall: [3, 4],
  glass: [2, 6],
  wood: [3, 5],
  brick: [7, 11],
  concrete: [12, 20],
  metal: [20, 30],
  shaft: [30, 38],
};
const CHANNELS: Record<Band, number[]> = {
  "2.4": [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13],
  "5": [36, 40, 44, 48, 52, 56, 60, 64, 100, 104, 108, 112, 116, 120, 124, 128, 132, 136, 140, 144, 149, 153, 157, 161, 165],
  "6": Array.from({ length: 59 }, (_, i) => 1 + i * 4),
};
const WIDTHS: Record<Band, number[]> = { "2.4": [20, 40], "5": [20, 40, 80, 160], "6": [20, 40, 80, 160, 320] };

function Section({ title, children, aside }: { title: ReactNode; children: ReactNode; aside?: ReactNode }) {
  return (
    <section className="side-section">
      <div className="side-head">
        <h3>{title}</h3>
        {aside}
      </div>
      {children}
    </section>
  );
}

function ToolButton({ tool, icon, label }: { tool: Tool; icon: ReactNode; label: string }) {
  const current = useSurvey((s) => s.tool);
  const set = useSurvey((s) => s.set);
  return (
    <button className="tool-btn" aria-pressed={current === tool} onClick={() => set({ tool, calibration: tool === "calibrate" ? { a: null, b: null } : useSurvey.getState().calibration })}>
      {icon}
      <span>{label}</span>
    </button>
  );
}

function Legend({ heatmap, layer }: { heatmap: Heatmap; layer: Layer }) {
  const t = useT();
  const lang = useLang();
  if (layer === "bestAp") {
    return (
      <ul className="legend-list">
        {heatmap.legend.map((l) => (
          <li key={l.index}>
            <span className="swatch" style={{ background: cssColor(colorFor("bestAp", l.index)) }} />
            <span title={l.detail}>{l.label}</span>
          </li>
        ))}
      </ul>
    );
  }
  const stops = legendStops(layer);
  const lo = stops[0].v;
  const hi = stops[stops.length - 1].v;
  const gradient = stops.map((s) => `${s.color} ${((s.v - lo) / (hi - lo)) * 100}%`).join(", ");
  const pct = fmtNumber(lang, heatmap.stats.coverageRatio * 100);
  const coverKey: Record<Exclude<Layer, "bestAp">, Key> = {
    signal: "survey.legend.covered",
    secondSignal: "survey.legend.covered",
    snr: "survey.legend.coveredSnr",
    apCount: "survey.legend.coveredCount",
    coChannel: "survey.legend.coveredCci",
    throughput: "survey.legend.coveredTput",
  };
  return (
    <div className="legend">
      <div className="legend-bar" style={{ background: `linear-gradient(to right, ${gradient})` }} />
      <div className="legend-ticks">
        {stops
          .filter((_, i) => stops.length <= 5 || i % 2 === 0 || i === stops.length - 1)
          .map((s) => (
            <span key={s.v} style={{ left: `${((s.v - lo) / (hi - lo)) * 100}%` }}>
              {s.v}
            </span>
          ))}
      </div>
      <div className="small muted" style={{ marginTop: 18 }}>
        {unitFor(layer)}
      </div>
      <div className="legend-stat">{t(coverKey[layer], { pct })}</div>
      {heatmap.stats.areaM2 != null && <div className="small muted">{t("survey.legend.area", { m2: fmtNumber(lang, heatmap.stats.areaM2) })}</div>}
    </div>
  );
}

function ApEditor({ ap }: { ap: VirtualAp }) {
  const t = useT();
  const update = useSurvey((s) => s.update);
  const set = useSurvey((s) => s.set);
  const patch = (fn: (a: VirtualAp) => VirtualAp) => update((p) => ({ ...p, aps: p.aps.map((a) => (a.id === ap.id ? fn(a) : a)) }));
  return (
    <div className="stack" style={{ gap: 10 }}>
      <label className="side-field">
        <span>{t("survey.ap.name")}</span>
        <input className="input" defaultValue={ap.name} key={ap.id} onBlur={(e) => patch((a) => ({ ...a, name: e.target.value || a.name }))} />
      </label>
      {ap.radios.map((r, i) => (
        <div key={i} className="radio-card">
          <label className="check" style={{ justifyContent: "space-between", width: "100%" }}>
            <span className="cell-strong">{t("survey.ap.radio", { band: r.band })}</span>
            <span className="row" style={{ gap: 6 }}>
              <input
                type="checkbox"
                checked={r.enabled}
                onChange={(e) => patch((a) => ({ ...a, radios: a.radios.map((x, j) => (j === i ? { ...x, enabled: e.target.checked } : x)) }))}
              />
              {t("survey.ap.enabled")}
            </span>
          </label>
          <div className="radio-grid">
            <label>
              <span>{t("survey.ap.channel")}</span>
              <select
                className="input"
                value={r.channel}
                onChange={(e) => patch((a) => ({ ...a, radios: a.radios.map((x, j) => (j === i ? { ...x, channel: Number(e.target.value) } : x)) }))}
              >
                {CHANNELS[r.band].map((c) => (
                  <option key={c}>{c}</option>
                ))}
              </select>
            </label>
            <label>
              <span>{t("survey.ap.width")}</span>
              <select
                className="input"
                value={r.widthMhz}
                onChange={(e) => patch((a) => ({ ...a, radios: a.radios.map((x, j) => (j === i ? { ...x, widthMhz: Number(e.target.value) } : x)) }))}
              >
                {WIDTHS[r.band].map((w) => (
                  <option key={w} value={w}>
                    {w} MHz
                  </option>
                ))}
              </select>
            </label>
            <label>
              <span>{t("survey.ap.power")}</span>
              <input
                className="input"
                type="number"
                min={0}
                max={36}
                value={r.eirpDbm}
                onChange={(e) => patch((a) => ({ ...a, radios: a.radios.map((x, j) => (j === i ? { ...x, eirpDbm: Number(e.target.value) } : x)) }))}
              />
            </label>
          </div>
        </div>
      ))}
      <button
        className="btn btn-ghost"
        onClick={() => {
          update((p) => ({ ...p, aps: p.aps.filter((a) => a.id !== ap.id) }));
          set({ selectedAp: null });
        }}
      >
        <Trash2 size={15} />
        {t("survey.ap.delete")}
      </button>
    </div>
  );
}

export function SurveySidebar({
  heatmap,
  isSim,
  request,
  suggestions,
  setSuggestions,
  finishWalk,
  measuring,
  onError,
}: {
  heatmap: Heatmap | null;
  isSim: boolean;
  request: Record<string, unknown> | null;
  suggestions: PlacementSuggestion[];
  setSuggestions: (s: PlacementSuggestion[]) => void;
  finishWalk: () => void;
  measuring: boolean;
  onError: (msg: string) => void;
}) {
  const t = useT();
  const lang = useLang();
  const s = useSurvey();
  const project = s.project!;
  const [distance, setDistance] = useState("");
  const [planBand, setPlanBand] = useState<Band>("5");
  const [planWidth, setPlanWidth] = useState(80);
  const [dfs, setDfs] = useState(true);
  const [plan, setPlan] = useState<ChannelPlan | null>(null);
  const [busy, setBusy] = useState(false);

  const ssidOptions = useMemo(() => {
    const count = new Map<string, number>();
    for (const smp of project.samples) for (const r of smp.readings) if (r.ssid) count.set(r.ssid, (count.get(r.ssid) ?? 0) + 1);
    return Array.from(count.entries())
      .sort((a, b) => b[1] - a[1])
      .slice(0, 12)
      .map(([ssid]) => ssid);
  }, [project.samples]);

  const selectedAp = project.aps.find((a) => a.id === s.selectedAp);
  const mpp = project.floorPlan.metersPerPx;
  const toolHelp: Partial<Record<Tool, Key>> = {
    measure: "survey.tool.measure.help",
    walk: isSim ? "survey.tool.walk.helpSim" : "survey.tool.walk.help",
    wall: "survey.tool.wall.help",
    ap: "survey.tool.ap.help",
    select: "survey.tool.select.help",
  };

  const save = async () => {
    await saveTextFile(`${project.name || "survey"}.whylag`, JSON.stringify(project), [{ name: "WhyLag survey", extensions: ["whylag"] }]);
  };
  const exportPng = async () => {
    const png = await renderSurveyPng(project, heatmap, { opacity: s.opacity });
    await saveDataUrl(`${project.name || "survey"}-${s.layer}.png`, png, [{ name: "PNG", extensions: ["png"] }]);
  };
  const applyScale = () => {
    const c = s.calibration;
    const meters = parseFloat(distance.replace(",", "."));
    if (!c.a || !c.b || !(meters > 0)) return;
    const px = Math.hypot(c.b.x - c.a.x, c.b.y - c.a.y);
    s.update((p) => ({ ...p, floorPlan: { ...p.floorPlan, metersPerPx: meters / px } }));
    s.set({ calibration: { a: null, b: null }, tool: s.mode === "measured" ? "measure" : "wall" });
    setDistance("");
  };
  const runPlan = async () => {
    setBusy(true);
    try {
      const r = await call<ChannelPlan>("survey.channelPlan", {
        widthPx: project.floorPlan.widthPx,
        heightPx: project.floorPlan.heightPx,
        metersPerPx: mpp,
        walls: project.walls,
        aps: project.aps,
        band: planBand,
        widthMhz: planWidth,
        allowDfs: dfs,
        external: [],
      });
      setPlan(r);
    } catch (e) {
      onError(String((e as Error).message ?? e));
    } finally {
      setBusy(false);
    }
  };
  const applyPlan = () => {
    if (!plan) return;
    s.update((p) => ({
      ...p,
      aps: p.aps.map((a) => {
        const asg = plan.assignments.find((x) => x.apId === a.id);
        if (!asg) return a;
        return { ...a, radios: a.radios.map((r) => (r.band === plan.band ? { ...r, channel: asg.to, widthMhz: plan.widthMhz } : r)) };
      }),
    }));
    setPlan(null);
  };
  const suggest = async () => {
    if (!request) return;
    setBusy(true);
    try {
      const r = await call<PlacementSuggestion[]>("survey.suggestAp", { ...request, layer: "signal", count: 3 });
      setSuggestions(r.filter((x) => x.coverageAfter > x.coverageBefore + 0.005));
      if (!r.length || r.every((x) => x.coverageAfter <= x.coverageBefore + 0.005)) onError(t("survey.suggest.none"));
    } catch (e) {
      onError(String((e as Error).message ?? e));
    } finally {
      setBusy(false);
    }
  };
  const addSuggested = () => {
    const sg = suggestions[0];
    if (!sg) return;
    const n = project.aps.length + 1;
    s.update((p) => ({
      ...p,
      aps: [
        ...p.aps,
        {
          id: `ap-${Date.now().toString(36)}`,
          name: `AP-${String(n).padStart(2, "0")}`,
          x: sg.x,
          y: sg.y,
          radios: [
            { band: "2.4", channel: 6, widthMhz: 20, eirpDbm: 14, enabled: true },
            { band: "5", channel: 36, widthMhz: 80, eirpDbm: 17, enabled: true },
          ],
        },
      ],
    }));
    setSuggestions([]);
  };
  const importDemoAps = async () => {
    const aps = await call<VirtualAp[]>("sim.aps");
    s.update((p) => ({ ...p, aps }));
  };

  return (
    <aside className="survey-side">
      <div className="side-top">
        <input
          className="side-name"
          aria-label="Survey name"
          defaultValue={project.name}
          key={project.createdMs}
          onBlur={(e) => s.update((p) => ({ ...p, name: e.target.value || p.name }))}
        />
        <div className="row" style={{ gap: 4 }}>
          <button className="btn btn-ghost btn-sm" onClick={s.undo} disabled={!s.undoStack.length} title={t("survey.undo")} aria-label={t("survey.undo")}>
            <Undo2 size={15} />
          </button>
          <button className="btn btn-ghost btn-sm" onClick={s.redo} disabled={!s.redoStack.length} title={t("survey.redo")} aria-label={t("survey.redo")}>
            <Redo2 size={15} />
          </button>
          <span className="spacer" />
          <button className="btn btn-sm" onClick={save}>
            <Save size={14} />
            {t("common.save")}
          </button>
          <button className="btn btn-sm" onClick={exportPng} title={t("survey.exportImage")} aria-label={t("survey.exportImage")}>
            <Download size={14} />
          </button>
          <button className="btn btn-ghost btn-sm" onClick={() => s.setProject(null)} title={t("survey.new")} aria-label={t("survey.new")}>
            <X size={15} />
          </button>
        </div>
      </div>

      <div className="side-scroll">
        <Segmented
          value={s.mode}
          onChange={(mode) => s.set({ mode, tool: mode === "measured" ? "measure" : "ap", layer: mode === "predicted" && s.layer === "throughput" ? "signal" : s.layer })}
          options={[
            { value: "measured", label: t("survey.mode.measured") },
            { value: "predicted", label: t("survey.mode.predicted") },
          ]}
        />

        <Section title={t("survey.tools")}>
          <div className="tool-grid">
            {s.mode === "measured" ? (
              <>
                <ToolButton tool="measure" icon={<Crosshair size={17} />} label={t("survey.tool.measure")} />
                <ToolButton tool="walk" icon={<Footprints size={17} />} label={t("survey.tool.walk")} />
              </>
            ) : (
              <>
                <ToolButton tool="wall" icon={<BrickWall size={17} />} label={t("survey.tool.wall")} />
                <ToolButton tool="ap" icon={<MapPinPlus size={17} />} label={t("survey.tool.ap")} />
              </>
            )}
            <ToolButton tool="calibrate" icon={<Ruler size={17} />} label={t("survey.tool.calibrate")} />
            <ToolButton tool="select" icon={<Hand size={17} />} label={t("survey.tool.select")} />
          </div>
          {toolHelp[s.tool] && <p className="side-help">{t(toolHelp[s.tool]!)}</p>}

          {s.mode === "measured" && s.tool === "measure" && (
            <div className="stack" style={{ gap: 6 }}>
              <label className="check small">
                <input type="checkbox" checked={s.measureThroughput} onChange={(e) => s.set({ measureThroughput: e.target.checked })} />
                {t("survey.tput.enable")}
              </label>
              {s.measureThroughput && !isSim && (
                <label className="side-field">
                  <span>{t("survey.tput.kind")}</span>
                  <select className="input" value={s.throughputKind} onChange={(e) => s.set({ throughputKind: e.target.value as "peer" | "iperf3" })}>
                    <option value="peer">{t("perf.peer.kindPeer")}</option>
                    <option value="iperf3">{t("perf.peer.kindIperf")}</option>
                  </select>
                </label>
              )}
              {s.measureThroughput && !isSim && (
                <label className="side-field">
                  <span>{t("survey.tput.host")}</span>
                  <input className="input" value={s.throughputHost} onChange={(e) => s.set({ throughputHost: e.target.value })} placeholder="192.168.1.20" />
                </label>
              )}
              {s.measureThroughput && !isSim && s.throughputKind === "peer" && (
                <label className="side-field">
                  <span>{t("perf.peer.code")}</span>
                  <input className="input" inputMode="numeric" value={s.throughputCode} onChange={(e) => s.set({ throughputCode: e.target.value })} placeholder="123 456" />
                </label>
              )}
              {s.measureThroughput && <p className="side-help">{t("survey.tput.help")}</p>}
            </div>
          )}

          {s.tool === "wall" && (
            <label className="side-field">
              <span>{t("survey.material")}</span>
              <select className="input" value={s.wallMaterial} onChange={(e) => s.set({ wallMaterial: e.target.value as Material })}>
                {MATERIALS.map((m) => (
                  <option key={m} value={m}>
                    {t(`survey.material.${m}` as Key)} ({MATERIAL_LOSS[m][0]}/{MATERIAL_LOSS[m][1]} dB)
                  </option>
                ))}
              </select>
            </label>
          )}

          {s.tool === "calibrate" && (
            <div className="stack" style={{ gap: 8 }}>
              <p className="side-help">{t("survey.scale.prompt")}</p>
              {s.calibration.a && s.calibration.b && (
                <>
                  <label className="side-field">
                    <span>{t("survey.scale.distance")}</span>
                    <input className="input" autoFocus inputMode="decimal" value={distance} onChange={(e) => setDistance(e.target.value)} onKeyDown={(e) => e.key === "Enter" && applyScale()} />
                  </label>
                  <button className="btn btn-primary" onClick={applyScale}>
                    {t("survey.scale.apply")}
                  </button>
                </>
              )}
            </div>
          )}

          {s.walk && (
            <div className="walk-banner">
              <span className="dot pulse-dot" aria-hidden />
              <span>{t("survey.walk.active", { n: project.samples.length })}</span>
              <button className="btn btn-primary btn-sm" onClick={finishWalk}>
                {t("survey.walk.finish")}
              </button>
            </div>
          )}
          {measuring && <div className="walk-banner">{t("survey.measuring")}</div>}

          <div className="side-meta">
            {mpp ? (
              <span>{t("survey.scale.value", { px: fmtNumber(lang, 1 / mpp, 1) })}</span>
            ) : (
              <span className="warn-text">{t("survey.scale.missing")}</span>
            )}
            {s.mode === "measured" ? (
              <span className="row" style={{ gap: 6 }}>
                {t("survey.samples", { n: project.samples.length })}
                {project.samples.length > 0 && (
                  <button
                    className="link-btn"
                    onClick={() => {
                      if (window.confirm(t("survey.clearSamples.confirm", { n: project.samples.length }))) s.update((p) => ({ ...p, samples: [] }));
                    }}
                  >
                    {t("survey.clearSamples")}
                  </button>
                )}
              </span>
            ) : (
              <span>{t("survey.walls", { n: project.walls.length })}</span>
            )}
          </div>
        </Section>

        {s.mode === "predicted" && (
          <Section title={selectedAp ? selectedAp.name : "AP"}>
            {selectedAp ? <ApEditor ap={selectedAp} /> : <p className="side-help">{t("survey.ap.none")}</p>}
            {isSim && project.aps.length === 0 && (
              <button className="btn" onClick={importDemoAps}>
                {t("survey.importDemoAps")}
              </button>
            )}
          </Section>
        )}

        <Section title={t("survey.view")}>
          <label className="side-field">
            <span>{t("survey.layer")}</span>
            <select className="input" value={s.layer} onChange={(e) => s.set({ layer: e.target.value as Layer })}>
              {LAYERS.filter((l) => s.mode === "measured" || l !== "throughput").map((l) => (
                <option key={l} value={l}>
                  {t(`survey.layer.${l}` as Key)}
                </option>
              ))}
            </select>
          </label>
          <p className="side-help">{t(`survey.layer.${s.layer}.help` as Key)}</p>

          {s.mode === "measured" && ssidOptions.length > 0 && (
            <div className="side-field">
              <span>{t("survey.networks")}</span>
              <div className="chips">
                {ssidOptions.map((ssid) => {
                  const on = s.ssids.includes(ssid);
                  return (
                    <button key={ssid} className="chip" aria-pressed={on} onClick={() => s.set({ ssids: on ? s.ssids.filter((x) => x !== ssid) : [...s.ssids, ssid] })}>
                      {ssid}
                    </button>
                  );
                })}
              </div>
              {s.ssids.length === 0 && <span className="small muted">{t("survey.networks.all")}</span>}
            </div>
          )}

          <div className="side-field">
            <span>{t("survey.bands")}</span>
            <div className="chips">
              {(["2.4", "5", "6"] as Band[]).map((b) => {
                const on = s.bands.includes(b);
                return (
                  <button key={b} className="chip" aria-pressed={on} onClick={() => s.set({ bands: on ? s.bands.filter((x) => x !== b) : [...s.bands, b] })}>
                    {b} GHz
                  </button>
                );
              })}
            </div>
          </div>

          <label className="side-field">
            <span>{t("survey.opacity")}</span>
            <input type="range" min={0.15} max={1} step={0.05} value={s.opacity} onChange={(e) => s.set({ opacity: Number(e.target.value) })} />
          </label>

          {s.mode === "measured" && (
            <>
              <label className="check small">
                <input type="checkbox" checked={s.showFoundAps} onChange={(e) => s.set({ showFoundAps: e.target.checked })} />
                {t("survey.found.show")}
              </label>
              <label className="check small">
                <input type="checkbox" checked={s.showRoams} onChange={(e) => s.set({ showRoams: e.target.checked })} />
                {t("survey.roams.show")}
              </label>
              {s.showFoundAps && (
                <p className="side-help">{s.foundAps.length ? t("survey.found.help", { n: s.foundAps.filter((ap) => !s.ssids.length || ap.ssids.some((x) => s.ssids.includes(x))).length }) : t("survey.found.none")}</p>
              )}
            </>
          )}

          {heatmap ? (
            <Legend heatmap={heatmap} layer={s.layer} />
          ) : (
            <p className="side-help">{s.mode === "measured" ? t("survey.legend.noData") : t("survey.legend.noAps")}</p>
          )}
        </Section>

        {s.mode === "predicted" && project.aps.length > 1 && (
          <Section title={t("survey.plan.title")}>
            <div className="row" style={{ gap: 8 }}>
              <Segmented<Band>
                value={planBand}
                onChange={(b) => {
                  setPlanBand(b);
                  setPlanWidth(b === "2.4" ? 20 : 80);
                  setPlan(null);
                }}
                options={(["2.4", "5", "6"] as Band[]).map((b) => ({ value: b, label: b }))}
              />
              <select className="input" value={planWidth} onChange={(e) => setPlanWidth(Number(e.target.value))} style={{ width: 96 }}>
                {WIDTHS[planBand].map((w) => (
                  <option key={w} value={w}>
                    {w} MHz
                  </option>
                ))}
              </select>
            </div>
            {planBand === "5" && (
              <label className="check small">
                <input type="checkbox" checked={dfs} onChange={(e) => setDfs(e.target.checked)} />
                {t("survey.plan.dfs")}
              </label>
            )}
            <button className="btn" onClick={runPlan} disabled={busy}>
              <Wand2 size={15} />
              {t("survey.plan.run")}
            </button>
            {plan && (
              <div className="stack" style={{ gap: 8 }}>
                <table className="data">
                  <tbody>
                    {plan.assignments.map((a) => (
                      <tr key={a.apId} style={{ cursor: "default" }}>
                        <td>{a.apName}</td>
                        <td className="num muted">{a.from ?? "—"}</td>
                        <td className="num cell-strong" style={{ color: a.from !== a.to ? "var(--accent)" : undefined }}>
                          {a.to}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                <p className="side-help">
                  {plan.costAfter < plan.costBefore - 1e-6
                    ? t("survey.plan.improve", { pct: fmtNumber(lang, (1 - plan.costAfter / Math.max(plan.costBefore, 1e-9)) * 100) })
                    : t("survey.plan.noChange")}
                </p>
                {plan.costAfter < plan.costBefore - 1e-6 && (
                  <button className="btn btn-primary" onClick={applyPlan}>
                    {t("survey.plan.apply")}
                  </button>
                )}
              </div>
            )}
          </Section>
        )}

        <Section title={t("survey.report.title")}>
          <p className="side-help">{t("survey.report.help")}</p>
          <button
            className="btn"
            onClick={() => {
              useApp.getState().setReportPreset(["survey"]);
              useApp.getState().setModule("reports");
            }}
          >
            <FileText size={15} />
            {t("survey.report.open")}
          </button>
        </Section>

        <Section title={t("survey.suggest.title")}>
          <div className="row" style={{ gap: 8 }}>
            <button className="btn" onClick={suggest} disabled={busy || !heatmap}>
              <MapPinPlus size={15} />
              {t("survey.suggest.run")}
            </button>
            {suggestions.length > 0 && (
              <button className="btn btn-ghost btn-sm" onClick={() => setSuggestions([])}>
                {t("survey.suggest.clear")}
              </button>
            )}
          </div>
          {suggestions.map((sg, i) => (
            <div key={i} className="small">
              {t("survey.suggest.result", {
                n: i + 1,
                from: fmtNumber(lang, sg.coverageBefore * 100),
                to: fmtNumber(lang, sg.coverageAfter * 100),
              })}
            </div>
          ))}
          {suggestions.length > 0 && s.mode === "predicted" && (
            <button className="btn btn-primary" onClick={addSuggested}>
              {t("survey.suggest.add")}
            </button>
          )}
        </Section>
      </div>
    </aside>
  );
}
