import { useCallback, useEffect, useLayoutEffect, useRef, useState, type PointerEvent as RPointerEvent } from "react";
import type { Heatmap, PlacementSuggestion, SurveyProject, Wall } from "../../types";
import { colorFor } from "./colors";
import { useSurvey } from "./surveyStore";
import { quality, qualityColor } from "../../lib/signal";

const WALL_COLORS: Record<Wall["material"], string> = {
  drywall: "#7d8794",
  glass: "#4aa3d4",
  wood: "#a0773f",
  brick: "#a2533e",
  concrete: "#3d4148",
  metal: "#43557a",
  shaft: "#1f2328",
};

export interface CanvasHandlers {
  onMeasure: (x: number, y: number) => void;
  onWalkPoint: (x: number, y: number) => void;
  onHover: (x: number, y: number) => void;
  onAddWall: (w: Wall) => void;
  onDeleteWall: (index: number) => void;
  onAddAp: (x: number, y: number) => void;
  onMoveAp: (id: string, x: number, y: number, final: boolean) => void;
  onCalibratePoint: (x: number, y: number) => void;
}

interface View {
  scale: number;
  tx: number;
  ty: number;
}

/** Points along the walk where the computer switched access points. */
function roamMarks(samples: SurveyProject["samples"]) {
  const out: { x: number; y: number; from: string; to: string }[] = [];
  let prev: string | null = null;
  for (const s of samples) {
    const cur = s.readings.find((x) => x.connected)?.bssid ?? null;
    if (cur && prev && cur !== prev) out.push({ x: s.x, y: s.y, from: prev, to: cur });
    if (cur) prev = cur;
  }
  return out;
}

/** Best "your network" signal of a sample, used to colour sample dots. */
function sampleSignal(s: SurveyProject["samples"][number], ssids: string[]) {
  const vals = s.readings.filter((r) => !ssids.length || ssids.includes(r.ssid)).map((r) => r.rssi);
  return vals.length ? Math.max(...vals) : null;
}

export function SurveyCanvas({
  project,
  heatmap,
  suggestions,
  handlers,
  walkCursor,
  pending,
}: {
  project: SurveyProject;
  heatmap: Heatmap | null;
  suggestions: PlacementSuggestion[];
  handlers: CanvasHandlers;
  walkCursor: { x: number; y: number } | null;
  pending: { x: number; y: number } | null;
}) {
  const host = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const svg = useRef<SVGSVGElement>(null);
  const [view, setView] = useState<View>({ scale: 1, tx: 0, ty: 0 });
  const [hover, setHover] = useState<{ x: number; y: number } | null>(null);
  const [wallStart, setWallStart] = useState<{ x: number; y: number } | null>(null);
  const [selectedWall, setSelectedWall] = useState<number | null>(null);
  const drag = useRef<{ kind: "pan" | "ap"; id?: string; sx: number; sy: number; view: View; moved: boolean } | null>(null);
  const pointers = useRef(new Map<number, { x: number; y: number }>());
  const pinch = useRef<{ dist: number; view: View; cx: number; cy: number } | null>(null);

  const { tool, opacity, ssids, walk, calibration, selectedAp, mode, foundAps, showRoams } = useSurvey();
  const fp = project.floorPlan;

  // Fit the plan into the view when a project loads or the host resizes first time.
  const fit = useCallback(() => {
    const el = host.current;
    if (!el) return;
    const s = Math.min((el.clientWidth - 24) / fp.widthPx, (el.clientHeight - 24) / fp.heightPx);
    setView({ scale: s, tx: (el.clientWidth - fp.widthPx * s) / 2, ty: (el.clientHeight - fp.heightPx * s) / 2 });
  }, [fp.widthPx, fp.heightPx]);
  useLayoutEffect(() => {
    fit();
  }, [fit, fp.imageDataUrl]);

  // Paint the heatmap at grid resolution; the browser smooths it when scaled.
  useEffect(() => {
    const c = canvas.current;
    if (!c) return;
    const ctx = c.getContext("2d");
    if (!ctx) return;
    if (!heatmap) {
      ctx.clearRect(0, 0, c.width, c.height);
      return;
    }
    c.width = heatmap.cols;
    c.height = heatmap.rows;
    const img = ctx.createImageData(heatmap.cols, heatmap.rows);
    for (let i = 0; i < heatmap.values.length; i++) {
      const v = heatmap.values[i];
      if (v == null) continue;
      const [r, g, b] = colorFor(heatmap.layer, v);
      img.data[i * 4] = r;
      img.data[i * 4 + 1] = g;
      img.data[i * 4 + 2] = b;
      img.data[i * 4 + 3] = 255;
    }
    ctx.putImageData(img, 0, 0);
  }, [heatmap]);

  const toPlan = useCallback(
    (clientX: number, clientY: number) => {
      const rect = host.current!.getBoundingClientRect();
      return { x: (clientX - rect.left - view.tx) / view.scale, y: (clientY - rect.top - view.ty) / view.scale };
    },
    [view],
  );

  const inside = (p: { x: number; y: number }) => p.x >= 0 && p.y >= 0 && p.x <= fp.widthPx && p.y <= fp.heightPx;

  // Escape cancels a wall chain; Delete removes the selected wall.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setWallStart(null);
        setSelectedWall(null);
      }
      if ((e.key === "Delete" || e.key === "Backspace") && selectedWall != null && (e.target as HTMLElement)?.tagName !== "INPUT") {
        handlers.onDeleteWall(selectedWall);
        setSelectedWall(null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [selectedWall, handlers]);

  useEffect(() => {
    if (tool !== "wall") setWallStart(null);
    if (tool !== "select") setSelectedWall(null);
  }, [tool]);

  const snap = (from: { x: number; y: number }, to: { x: number; y: number }, shift: boolean) => {
    if (!shift) return to;
    return Math.abs(to.x - from.x) > Math.abs(to.y - from.y) ? { x: to.x, y: from.y } : { x: from.x, y: to.y };
  };

  const onWheel = (e: React.WheelEvent) => {
    const rect = host.current!.getBoundingClientRect();
    const cx = e.clientX - rect.left;
    const cy = e.clientY - rect.top;
    const factor = Math.exp(-e.deltaY * (e.ctrlKey ? 0.01 : 0.0015));
    setView((v) => {
      const scale = Math.min(12, Math.max(0.1, v.scale * factor));
      const k = scale / v.scale;
      return { scale, tx: cx - (cx - v.tx) * k, ty: cy - (cy - v.ty) * k };
    });
  };

  const onPointerDown = (e: RPointerEvent) => {
    (e.target as Element).setPointerCapture?.(e.pointerId);
    pointers.current.set(e.pointerId, { x: e.clientX, y: e.clientY });
    if (pointers.current.size === 2) {
      const [a, b] = Array.from(pointers.current.values());
      pinch.current = { dist: Math.hypot(a.x - b.x, a.y - b.y), view, cx: (a.x + b.x) / 2, cy: (a.y + b.y) / 2 };
      drag.current = null;
      return;
    }
    const p = toPlan(e.clientX, e.clientY);
    const apHit = (e.target as Element).closest?.("[data-ap]")?.getAttribute("data-ap");
    if (apHit && (tool === "ap" || tool === "select")) {
      useSurvey.getState().set({ selectedAp: apHit });
      drag.current = { kind: "ap", id: apHit, sx: p.x, sy: p.y, view, moved: false };
      return;
    }
    if (e.button === 1 || e.button === 2 || tool === "select") {
      drag.current = { kind: "pan", sx: e.clientX, sy: e.clientY, view, moved: false };
    }
  };

  const onPointerMove = (e: RPointerEvent) => {
    if (pointers.current.has(e.pointerId)) pointers.current.set(e.pointerId, { x: e.clientX, y: e.clientY });
    if (pinch.current && pointers.current.size === 2) {
      const [a, b] = Array.from(pointers.current.values());
      const rect = host.current!.getBoundingClientRect();
      const dist = Math.hypot(a.x - b.x, a.y - b.y);
      const k = dist / pinch.current.dist;
      const v0 = pinch.current.view;
      const scale = Math.min(12, Math.max(0.1, v0.scale * k));
      const cx = (a.x + b.x) / 2 - rect.left;
      const cy = (a.y + b.y) / 2 - rect.top;
      const ox = pinch.current.cx - rect.left;
      const oy = pinch.current.cy - rect.top;
      setView({ scale, tx: cx - (ox - v0.tx) * (scale / v0.scale), ty: cy - (oy - v0.ty) * (scale / v0.scale) });
      return;
    }
    const p = toPlan(e.clientX, e.clientY);
    setHover(p);
    const d = drag.current;
    if (d?.kind === "pan") {
      const dx = e.clientX - d.sx;
      const dy = e.clientY - d.sy;
      if (Math.abs(dx) + Math.abs(dy) > 3) d.moved = true;
      setView({ ...d.view, tx: d.view.tx + dx, ty: d.view.ty + dy });
    } else if (d?.kind === "ap" && d.id) {
      d.moved = true;
      handlers.onMoveAp(d.id, Math.min(fp.widthPx, Math.max(0, p.x)), Math.min(fp.heightPx, Math.max(0, p.y)), false);
    }
    if (tool === "walk" && walk && inside(p)) handlers.onHover(p.x, p.y);
  };

  const onPointerUp = (e: RPointerEvent) => {
    pointers.current.delete(e.pointerId);
    if (pinch.current) {
      if (pointers.current.size < 2) pinch.current = null;
      return;
    }
    const d = drag.current;
    drag.current = null;
    const p = toPlan(e.clientX, e.clientY);
    if (d?.kind === "ap" && d.id) {
      if (d.moved) handlers.onMoveAp(d.id, Math.min(fp.widthPx, Math.max(0, p.x)), Math.min(fp.heightPx, Math.max(0, p.y)), true);
      return;
    }
    if (d?.kind === "pan" && d.moved) return;
    if (e.button !== 0 || !inside(p)) return;
    switch (tool) {
      case "measure":
        if (!pending) handlers.onMeasure(p.x, p.y);
        break;
      case "walk":
        handlers.onWalkPoint(p.x, p.y);
        break;
      case "calibrate":
        handlers.onCalibratePoint(p.x, p.y);
        break;
      case "wall": {
        if (!wallStart) {
          setWallStart(p);
        } else {
          const end = snap(wallStart, p, e.shiftKey);
          if (Math.hypot(end.x - wallStart.x, end.y - wallStart.y) > 3) {
            handlers.onAddWall({ x1: wallStart.x, y1: wallStart.y, x2: end.x, y2: end.y, material: useSurvey.getState().wallMaterial });
          }
          setWallStart(end);
        }
        break;
      }
      case "ap":
        handlers.onAddAp(p.x, p.y);
        break;
      case "select": {
        const hit = (e.target as Element).closest?.("[data-wall]")?.getAttribute("data-wall");
        setSelectedWall(hit != null ? Number(hit) : null);
        break;
      }
    }
  };

  const s = view.scale;
  const r = (px: number) => px / s; // constant on-screen size for markers
  const wallDraft = tool === "wall" && wallStart && hover ? snap(wallStart, hover, false) : null;
  const cursor = { measure: "crosshair", walk: "crosshair", calibrate: "crosshair", wall: "crosshair", ap: "copy", select: "grab" }[tool];
  const pathPts = walk?.waypoints ?? [];

  return (
    <div
      ref={host}
      className="survey-canvas"
      onWheel={onWheel}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerLeave={() => setHover(null)}
      onContextMenu={(e) => {
        e.preventDefault();
        setWallStart(null);
      }}
      onDoubleClick={fit}
      style={{ cursor }}
    >
      <div className="survey-stage" style={{ transform: `translate(${view.tx}px, ${view.ty}px) scale(${s})`, width: fp.widthPx, height: fp.heightPx }}>
        <img src={fp.imageDataUrl} width={fp.widthPx} height={fp.heightPx} alt={fp.name} draggable={false} className="survey-plan" />
        <canvas ref={canvas} className="survey-heat" style={{ width: fp.widthPx, height: fp.heightPx, opacity: heatmap ? opacity : 0 }} />
        <svg ref={svg} className="survey-overlay" width={fp.widthPx} height={fp.heightPx} viewBox={`0 0 ${fp.widthPx} ${fp.heightPx}`}>
          {project.walls.map((w, i) => (
            <line
              key={i}
              data-wall={i}
              x1={w.x1}
              y1={w.y1}
              x2={w.x2}
              y2={w.y2}
              stroke={selectedWall === i ? "var(--accent)" : WALL_COLORS[w.material]}
              strokeWidth={r(selectedWall === i ? 6 : mode === "predicted" || tool === "wall" || tool === "select" ? 4 : 0)}
              strokeLinecap="round"
              opacity={mode === "predicted" || tool === "wall" || tool === "select" ? 0.9 : 0}
              style={{ pointerEvents: tool === "select" ? "stroke" : "none" }}
            />
          ))}
          {wallDraft && wallStart && (
            <line x1={wallStart.x} y1={wallStart.y} x2={wallDraft.x} y2={wallDraft.y} stroke="var(--accent)" strokeWidth={r(3)} strokeDasharray={`${r(6)} ${r(4)}`} />
          )}

          {mode === "measured" &&
            project.samples.map((smp, i) => {
              const v = sampleSignal(smp, ssids);
              return (
                <circle
                  key={i}
                  cx={smp.x}
                  cy={smp.y}
                  r={r(3.2)}
                  fill={v == null ? "var(--ink-3)" : qualityColor[quality(v)]}
                  stroke="#fff"
                  strokeWidth={r(1.2)}
                  style={{ pointerEvents: "none" }}
                />
              );
            })}

          {mode === "measured" && showRoams && roamMarks(project.samples).map((m, i) => (
            <g key={`roam-${i}`} transform={`translate(${m.x} ${m.y})`} style={{ pointerEvents: "none" }}>
              <title>{`${m.from} → ${m.to}`}</title>
              <rect x={-r(7)} y={-r(7)} width={r(14)} height={r(14)} rx={r(3)} transform="rotate(45)" fill="#d9632b" stroke="#fff" strokeWidth={r(1.5)} />
              <path d={`M${-r(3.5)},${-r(1.2)} H${r(3.5)} l${-r(1.6)},${-r(1.6)} M${r(3.5)},${r(1.2)} H${-r(3.5)} l${r(1.6)},${r(1.6)}`} fill="none" stroke="#fff" strokeWidth={r(1.2)} strokeLinecap="round" />
            </g>
          ))}

          {mode === "measured" && foundAps.filter((ap) => !ssids.length || ap.ssids.some((x) => ssids.includes(x))).map((ap) => (
            <g key={ap.key} transform={`translate(${ap.x} ${ap.y})`} style={{ pointerEvents: "none" }} opacity={ap.confidence === "high" ? 1 : 0.6}>
              <title>{`${ap.ssids.join(", ") || ap.key}\n${ap.bssids.join("\n")}`}</title>
              <circle r={r(13)} fill="rgba(23,32,43,0.08)" stroke="#17202b" strokeWidth={r(1.6)} strokeDasharray={`${r(4)} ${r(3)}`} />
              <circle r={r(4)} fill="#17202b" />
              <text y={r(26)} textAnchor="middle" fontSize={r(11)} fontWeight={600} fill="#17202b" stroke="#fff" strokeWidth={r(3)} paintOrder="stroke">
                {(ap.ssids[0] ?? ap.key).slice(0, 18)} · {ap.channels.join("/")}
              </text>
            </g>
          ))}

          {pathPts.length > 0 && (
            <polyline
              points={[...pathPts, ...(walkCursor ? [walkCursor] : [])].map((p) => `${p.x},${p.y}`).join(" ")}
              fill="none"
              stroke="var(--accent)"
              strokeWidth={r(2.5)}
              strokeDasharray={`${r(7)} ${r(5)}`}
              style={{ pointerEvents: "none" }}
            />
          )}
          {walkCursor && walk && <circle cx={walkCursor.x} cy={walkCursor.y} r={r(7)} fill="var(--accent)" className="pulse" style={{ pointerEvents: "none" }} />}

          {project.aps.map((ap) => {
            const sel = selectedAp === ap.id;
            const enabled = ap.radios.some((x) => x.enabled);
            return (
              <g key={ap.id} data-ap={ap.id} transform={`translate(${ap.x} ${ap.y})`} style={{ cursor: tool === "ap" || tool === "select" ? "move" : "default" }}>
                {sel && <circle r={r(16)} fill="none" stroke="var(--accent)" strokeWidth={r(2)} />}
                <circle r={r(10)} fill={enabled ? "#17202b" : "#7d8794"} stroke="#fff" strokeWidth={r(2)} />
                <path d={`M${-r(4.5)},${r(1)} a${r(6)},${r(6)} 0 0 1 ${r(9)},0 M${-r(2.2)},${r(3)} a${r(3)},${r(3)} 0 0 1 ${r(4.4)},0`} fill="none" stroke="#fff" strokeWidth={r(1.4)} strokeLinecap="round" />
                <text y={r(24)} textAnchor="middle" fontSize={r(11.5)} fontWeight={600} fill="#17202b" stroke="#fff" strokeWidth={r(3)} paintOrder="stroke">
                  {ap.name}
                </text>
              </g>
            );
          })}

          {suggestions.map((sg, i) => (
            <g key={i} transform={`translate(${sg.x} ${sg.y})`} style={{ pointerEvents: "none" }}>
              <circle r={r(18)} fill="rgba(36,51,201,0.12)" stroke="var(--accent)" strokeWidth={r(2)} strokeDasharray={`${r(5)} ${r(4)}`} />
              <path d={`M${-r(6)},0 H${r(6)} M0,${-r(6)} V${r(6)}`} stroke="var(--accent)" strokeWidth={r(2.4)} strokeLinecap="round" />
              <text y={r(32)} textAnchor="middle" fontSize={r(12)} fontWeight={700} fill="var(--accent)" stroke="#fff" strokeWidth={r(3)} paintOrder="stroke">
                {i + 1}. +{Math.round((sg.coverageAfter - sg.coverageBefore) * 100)}%
              </text>
            </g>
          ))}

          {calibration.a && (
            <g style={{ pointerEvents: "none" }}>
              <line
                x1={calibration.a.x}
                y1={calibration.a.y}
                x2={(calibration.b ?? hover ?? calibration.a).x}
                y2={(calibration.b ?? hover ?? calibration.a).y}
                stroke="#d9632b"
                strokeWidth={r(2.5)}
              />
              {[calibration.a, calibration.b].filter(Boolean).map((p, i) => (
                <circle key={i} cx={p!.x} cy={p!.y} r={r(5)} fill="#d9632b" stroke="#fff" strokeWidth={r(1.5)} />
              ))}
            </g>
          )}

          {pending && <circle cx={pending.x} cy={pending.y} r={r(12)} fill="none" stroke="var(--accent)" strokeWidth={r(2.5)} className="pulse" />}
        </svg>
      </div>
    </div>
  );
}
