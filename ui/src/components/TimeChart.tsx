import { useEffect, useRef } from "react";
import uPlot from "uplot";
import { useApp } from "../store";

export interface Series {
  label: string;
  color: string;
  /** Points as [timestampMs, value]. */
  points: [number, number | null][];
  width?: number;
  dash?: number[];
}

function cssVar(name: string) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || "#888";
}

/**
 * Time series chart on uPlot (fast canvas rendering, fine for hours of samples).
 * All series are aligned to the union of their timestamps.
 */
export function TimeChart({
  series,
  height = 220,
  yRange,
  yLabel,
  ariaLabel,
  xTime = true,
}: {
  xTime?: boolean;
  series: Series[];
  height?: number;
  yRange?: [number, number];
  yLabel?: string;
  ariaLabel: string;
}) {
  const host = useRef<HTMLDivElement>(null);
  const plot = useRef<uPlot | null>(null);
  const theme = useApp((s) => s.theme);
  const key = series.map((s) => `${s.label}|${s.color}`).join(";");
  const latest = useRef({ series, xTime });
  latest.current = { series, xTime };

  const pushData = (u: uPlot) => {
    const { series, xTime } = latest.current;
    const ts = Array.from(new Set(series.flatMap((s) => s.points.map((p) => p[0])))).sort((a, b) => a - b);
    const index = new Map(ts.map((t, i) => [t, i]));
    const cols = series.map((s) => {
      const col: (number | null)[] = new Array(ts.length).fill(null);
      for (const [t, v] of s.points) col[index.get(t)!] = v;
      return col;
    });
    u.setData([xTime ? ts.map((t) => t / 1000) : ts, ...cols] as uPlot.AlignedData);
  };

  // (Re)create the chart when the set of series or the theme changes.
  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const ink3 = cssVar("--ink-3");
    const rule = cssVar("--rule");
    const opts: uPlot.Options = {
      width: el.clientWidth || 600,
      height,
      legend: { show: series.length > 0, live: false },
      cursor: { drag: { x: false, y: false } },
      scales: {
        x: { time: xTime },
        y: yRange ? { range: () => yRange } : { auto: true },
      },
      axes: [
        { stroke: ink3, grid: { stroke: rule, width: 1 }, ticks: { stroke: rule }, ...(xTime ? {} : { values: (_u: uPlot, vals: number[]) => vals.map((v) => `${v} s`) }) },
        { stroke: ink3, grid: { stroke: rule, width: 1 }, ticks: { stroke: rule }, label: yLabel, size: 48 },
      ],
      series: [
        {},
        ...series.map((s) => ({ label: s.label, stroke: s.color, width: s.width ?? 1.75, dash: s.dash, spanGaps: false, points: { show: false } })),
      ],
    };
    const u = new uPlot(opts, [[]], el);
    plot.current = u;
    pushData(u);
    const ro = new ResizeObserver(() => u.setSize({ width: el.clientWidth, height }));
    ro.observe(el);
    return () => {
      ro.disconnect();
      u.destroy();
      plot.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, theme, height, yLabel, yRange?.[0], yRange?.[1], xTime]);

  // Push data without recreating.
  useEffect(() => {
    if (plot.current) pushData(plot.current);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [series, xTime]);

  return <div ref={host} role="img" aria-label={ariaLabel} style={{ width: "100%", minHeight: height }} />;
}
