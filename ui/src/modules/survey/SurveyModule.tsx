import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { FolderOpen, ImagePlus, RotateCcw, Sparkles } from "lucide-react";
import { call, openFile, type CallError } from "../../api";
import { peerErrorKey } from "../../lib/peer";
import { useApp } from "../../store";
import { useT } from "../../i18n";
import type { Heatmap, PlacementSuggestion, Reading, Sample, ScanResult, SimInfo, SurveyProject, VirtualAp, Wall } from "../../types";
import { Notice } from "../../components/ui";
import { SurveyCanvas, type CanvasHandlers } from "./SurveyCanvas";
import { SurveySidebar } from "./SurveySidebar";
import { isProject, newProject, useSurvey } from "./surveyStore";

function toReadings(scan: ScanResult): Reading[] {
  return scan.networks.map((n) => ({
    bssid: n.bssid,
    ssid: n.ssid,
    rssi: n.rssi,
    channel: n.channel,
    band: n.band,
    widthMhz: n.widthMhz,
    noise: n.noise,
    security: n.security,
  }));
}

/** Loads an uploaded image, downscaling very large plans to keep the app fast. */
async function prepareImage(dataUrl: string): Promise<{ url: string; w: number; h: number }> {
  const img = await new Promise<HTMLImageElement>((resolve, reject) => {
    const i = new Image();
    i.onload = () => resolve(i);
    i.onerror = reject;
    i.src = dataUrl;
  });
  const w = img.naturalWidth || 1200;
  const h = img.naturalHeight || 800;
  const max = 3200;
  if (Math.max(w, h) <= max) return { url: dataUrl, w, h };
  const k = max / Math.max(w, h);
  const c = document.createElement("canvas");
  c.width = Math.round(w * k);
  c.height = Math.round(h * k);
  c.getContext("2d")!.drawImage(img, 0, 0, c.width, c.height);
  return { url: c.toDataURL("image/png"), w: c.width, h: c.height };
}

function StartScreen({ onError }: { onError: (msg: string) => void }) {
  const t = useT();
  const setProject = useSurvey((s) => s.setProject);
  const [autosave, setAutosave] = useState<SurveyProject | null>(null);

  useEffect(() => {
    call<string | null>("project.loadAutosave")
      .then((text) => {
        if (!text) return;
        const p = JSON.parse(text);
        if (isProject(p)) setAutosave(p);
      })
      .catch(() => {});
  }, []);

  const upload = async () => {
    const f = await openFile("image/png,image/jpeg,image/svg+xml,image/webp", true);
    if (!f) return;
    try {
      const img = await prepareImage(f.content);
      setProject(newProject(f.name.replace(/\.[^.]+$/, ""), img.url, img.w, img.h, null));
      useSurvey.getState().set({ tool: "calibrate", calibration: { a: null, b: null } });
    } catch {
      onError(t("survey.invalidFile"));
    }
  };

  const demo = async () => {
    const info = await call<SimInfo>("sim.info");
    const fp = info.floorPlan;
    setProject(newProject(fp.name, fp.imageDataUrl, fp.widthPx, fp.heightPx, fp.metersPerPx, fp.wallsPx));
    useSurvey.getState().set({ tool: "walk", mode: "measured", ssids: ["Acme-Office"] });
  };

  const open = async () => {
    const f = await openFile(".whylag,application/json");
    if (!f) return;
    try {
      const p = JSON.parse(f.content);
      if (!isProject(p)) throw new Error("not a project");
      setProject(p);
    } catch {
      onError(t("survey.invalidFile"));
    }
  };

  return (
    <div className="survey-start">
      <h2 className="survey-start-title">{t("survey.start.title")}</h2>
      <p className="survey-start-body">{t("survey.start.body")}</p>
      <div className="survey-start-actions">
        <button className="start-card" onClick={upload}>
          <ImagePlus size={22} />
          <span className="start-card-title">{t("survey.start.upload")}</span>
          <span className="muted small">{t("survey.start.uploadHint")}</span>
        </button>
        <button className="start-card" onClick={demo}>
          <Sparkles size={22} />
          <span className="start-card-title">{t("survey.start.demo")}</span>
          <span className="muted small">{t("survey.start.demoHint")}</span>
        </button>
        <button className="start-card" onClick={open}>
          <FolderOpen size={22} />
          <span className="start-card-title">{t("survey.start.open")}</span>
          <span className="muted small">.whylag</span>
        </button>
      </div>
      {autosave && (
        <button className="btn" onClick={() => setProject(autosave)} style={{ marginTop: 18 }}>
          <RotateCcw size={15} />
          {t("survey.start.restore")}: {autosave.name} ({t("survey.samples", { n: autosave.samples.length })})
        </button>
      )}
    </div>
  );
}

export function SurveyModule() {
  const t = useT();
  const project = useSurvey((s) => s.project);
  const { mode, layer, ssids, bands, walk, update, set } = useSurvey();
  const threshold = useApp((s) => s.settings?.coverageThresholdDbm ?? -67);
  const lastScan = useApp((s) => s.scan);
  const [heatmap, setHeatmap] = useState<Heatmap | null>(null);
  const [suggestions, setSuggestions] = useState<PlacementSuggestion[]>([]);
  const [pending, setPending] = useState<{ x: number; y: number } | null>(null);
  const [walkCursor, setWalkCursor] = useState<{ x: number; y: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const isSim = lastScan?.source === "simulator";
  const hoverRef = useRef<{ x: number; y: number } | null>(null);

  // Default the network filter to the network you are connected to.
  useEffect(() => {
    if (!project || ssids.length) return;
    const connected = lastScan?.interfaces.find((i) => i.state === "connected")?.ssid;
    if (connected) set({ ssids: [connected] });
  }, [project, ssids.length, lastScan, set]);

  const request = useMemo(() => {
    if (!project) return null;
    return {
      mode,
      layer,
      widthPx: project.floorPlan.widthPx,
      heightPx: project.floorPlan.heightPx,
      metersPerPx: project.floorPlan.metersPerPx,
      samples: mode === "measured" ? project.samples : [],
      walls: project.walls,
      aps: mode === "predicted" ? project.aps : [],
      filter: { ssids: mode === "measured" ? ssids : [], bands },
      thresholdDbm: threshold,
      reachM: 5,
    };
  }, [project, mode, layer, ssids, bands, threshold]);

  // Recompute the heatmap when anything relevant changes. At most one request is
  // in flight; while it runs, newer changes collapse into a single follow-up.
  const latest = useRef(request);
  latest.current = request;
  const inFlight = useRef(false);
  const dirty = useRef(false);
  const compute = useCallback(async () => {
    if (inFlight.current) {
      dirty.current = true;
      return;
    }
    inFlight.current = true;
    try {
      do {
        dirty.current = false;
        const req = latest.current;
        if (!req) break;
        const hasData = req.mode === "measured" ? req.samples.length > 0 : req.aps.length > 0;
        if (!hasData) {
          setHeatmap(null);
          continue;
        }
        try {
          const h = await call<Heatmap>("survey.heatmap", req);
          if (latest.current?.mode === h.mode) setHeatmap(h);
        } catch (e) {
          setError(String((e as Error).message ?? e));
        }
      } while (dirty.current);
    } finally {
      inFlight.current = false;
    }
  }, []);
  useEffect(() => {
    if (!request) return;
    const id = setTimeout(compute, 120);
    return () => clearTimeout(id);
  }, [request, compute]);

  // Suggestions are stale once the data changes.
  useEffect(() => setSuggestions([]), [project?.samples.length, project?.aps.length, mode]);

  const addSamples = useCallback(
    (samples: Sample[]) => {
      if (!samples.length) return;
      update((p) => ({ ...p, samples: [...p.samples, ...samples] }));
    },
    [update],
  );

  // ----- Walk mode: continuous scanning while the user moves -----
  const walkRef = useRef(walk);
  walkRef.current = walk;
  useEffect(() => {
    if (!walk) return;
    let alive = true;
    let timer: ReturnType<typeof setTimeout>;
    const tick = async () => {
      try {
        const pos = hoverRef.current;
        if (isSim && pos) await call("sim.setPosition", { xPx: pos.x, yPx: pos.y });
        const scan = await call<ScanResult>("wifi.scan", { trigger: true, minGapMs: 4000 });
        if (!alive || !walkRef.current) return;
        const readings = toReadings(scan);
        if (isSim && pos) {
          // The simulated laptop is exactly where the cursor is.
          addSamples([{ x: pos.x, y: pos.y, tMs: scan.timestampMs, readings }]);
        } else {
          set({ walk: { ...walkRef.current, pending: [...walkRef.current.pending, { t: Date.now(), readings }] } });
        }
      } catch {
        /* keep walking; a missed scan is just a gap */
      }
      if (alive) timer = setTimeout(tick, isSim ? 450 : 1200);
    };
    tick();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
    // Only (re)start when a walk begins or ends.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [!!walk, isSim]);

  /** Spreads scans taken between two waypoints along the segment by time. */
  const placePending = (w: NonNullable<typeof walk>, upTo: { x: number; y: number; t: number }) => {
    const from = w.waypoints[w.waypoints.length - 1];
    const span = Math.max(1, upTo.t - from.t);
    const placed: Sample[] = [];
    const rest: typeof w.pending = [];
    for (const p of w.pending) {
      if (p.t > upTo.t) {
        rest.push(p);
        continue;
      }
      const f = Math.min(1, Math.max(0, (p.t - from.t) / span));
      placed.push({ x: from.x + (upTo.x - from.x) * f, y: from.y + (upTo.y - from.y) * f, tMs: p.t, readings: p.readings });
    }
    return { placed, rest };
  };

  const finishWalk = () => {
    const w = useSurvey.getState().walk;
    if (w && !isSim && w.waypoints.length) {
      // Scans after the last click belong to where the walk ended.
      const last = w.waypoints[w.waypoints.length - 1];
      addSamples(w.pending.map((p) => ({ x: last.x, y: last.y, tMs: p.t, readings: p.readings })));
    }
    set({ walk: null });
    setWalkCursor(null);
  };

  const handlers: CanvasHandlers = {
    onMeasure: async (x, y) => {
      setPending({ x, y });
      try {
        await call("sim.setPosition", { xPx: x, yPx: y });
        const scan = await call<ScanResult>("wifi.scanFresh", { waitMs: 3500 });
        const { measureThroughput, throughputHost, throughputCode } = useSurvey.getState();
        let throughputMbps: number | null = null;
        if (measureThroughput && (isSim || throughputHost.trim())) {
          try {
            const r = await call<{ downloadMbps: number }>("survey.throughput", { host: throughputHost.trim(), code: throughputCode });
            throughputMbps = r.downloadMbps;
          } catch (e) {
            setError(t(peerErrorKey((e as CallError).code)));
          }
        }
        addSamples([{ x, y, tMs: scan.timestampMs, readings: toReadings(scan), throughputMbps }]);
      } catch (e) {
        setError(String((e as Error).message ?? e));
      } finally {
        setPending(null);
      }
    },
    onWalkPoint: (x, y) => {
      const now = Date.now();
      const w = useSurvey.getState().walk;
      hoverRef.current = { x, y };
      if (!w) {
        set({ walk: { waypoints: [{ x, y, t: now }], pending: [] } });
        setWalkCursor({ x, y });
        return;
      }
      const point = { x, y, t: now };
      if (isSim) {
        set({ walk: { ...w, waypoints: [...w.waypoints, point] } });
        return;
      }
      const { placed, rest } = placePending(w, point);
      addSamples(placed);
      set({ walk: { waypoints: [...w.waypoints, point], pending: rest } });
    },
    onHover: (x, y) => {
      hoverRef.current = { x, y };
      setWalkCursor({ x, y });
    },
    onAddWall: (wall: Wall) => update((p) => ({ ...p, walls: [...p.walls, wall] })),
    onDeleteWall: (index) => update((p) => ({ ...p, walls: p.walls.filter((_, i) => i !== index) })),
    onAddAp: (x, y) => {
      const n = (project?.aps.length ?? 0) + 1;
      const id = `ap-${Date.now().toString(36)}`;
      const ap: VirtualAp = {
        id,
        name: `AP-${String(n).padStart(2, "0")}`,
        x,
        y,
        radios: [
          { band: "2.4", channel: [1, 6, 11][(n - 1) % 3], widthMhz: 20, eirpDbm: 14, enabled: true },
          { band: "5", channel: [36, 52, 100, 116, 132, 149][(n - 1) % 6], widthMhz: 80, eirpDbm: 17, enabled: true },
        ],
      };
      update((p) => ({ ...p, aps: [...p.aps, ap] }));
      set({ selectedAp: id });
    },
    onMoveAp: (id, x, y, final) => {
      const s = useSurvey.getState();
      if (!s.project) return;
      const moved = { ...s.project, aps: s.project.aps.map((a) => (a.id === id ? { ...a, x, y } : a)) };
      // Record one undo step per drag, not per mouse move.
      if (final) s.update(() => moved);
      else useSurvey.setState({ project: moved });
    },
    onCalibratePoint: (x, y) => {
      const c = useSurvey.getState().calibration;
      if (!c.a || c.b) set({ calibration: { a: { x, y }, b: null } });
      else set({ calibration: { a: c.a, b: { x, y } } });
    },
  };

  if (!project) {
    return (
      <div className="stack">
        {error && <Notice tone="crit">{error}</Notice>}
        <StartScreen onError={setError} />
      </div>
    );
  }

  return (
    <div className="survey">
      <div className="survey-main">
        {error && (
          <div className="survey-error">
            <Notice tone="crit" action={<button className="btn btn-sm" onClick={() => setError(null)}>{t("common.close")}</button>}>
              {error}
            </Notice>
          </div>
        )}
        <SurveyCanvas project={project} heatmap={heatmap} suggestions={suggestions} handlers={handlers} walkCursor={walkCursor} pending={pending} />
      </div>
      <SurveySidebar
        heatmap={heatmap}
        isSim={isSim}
        request={request}
        suggestions={suggestions}
        setSuggestions={setSuggestions}
        finishWalk={finishWalk}
        measuring={!!pending}
        onError={setError}
      />
    </div>
  );
}
