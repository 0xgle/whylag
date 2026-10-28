import { useMemo } from "react";
import type { Band, Bss } from "../../types";
import { bssRange, displaySsid, seriesColor } from "../../lib/signal";
import { useWidth } from "../../lib/useWidth";

const AXIS: Record<Band, { lo: number; hi: number; ticks: { f: number; label: string }[] }> = {
  "2.4": {
    lo: 2398,
    hi: 2496,
    ticks: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14].map((c) => ({ f: c === 14 ? 2484 : 2407 + c * 5, label: String(c) })),
  },
  "5": {
    lo: 5160,
    hi: 5900,
    ticks: [36, 52, 64, 100, 116, 132, 144, 149, 165].map((c) => ({ f: 5000 + c * 5, label: String(c) })),
  },
  "6": {
    lo: 5940,
    hi: 7130,
    ticks: [1, 33, 65, 97, 129, 161, 193, 225].map((c) => ({ f: 5950 + c * 5, label: String(c) })),
  },
};

const TOP = -20;
const BOTTOM = -100;

export function ChannelGraph({
  band,
  networks,
  selected,
  onSelect,
  hiddenLabel,
  threshold,
}: {
  band: Band;
  networks: Bss[];
  selected: Set<string>;
  onSelect: (bssid: string) => void;
  hiddenLabel: string;
  threshold: number;
}) {
  const [host, W] = useWidth<HTMLDivElement>();
  const H = 250;
  const pad = { l: 40, r: 12, t: 12, b: 28 };
  const axis = AXIS[band];
  const x = (f: number) => pad.l + ((f - axis.lo) / (axis.hi - axis.lo)) * (W - pad.l - pad.r);
  const y = (dbm: number) => pad.t + ((TOP - dbm) / (TOP - BOTTOM)) * (H - pad.t - pad.b);

  const items = useMemo(
    () =>
      networks
        .filter((n) => n.band === band)
        .map((n) => ({ n, range: bssRange(n)! }))
        .filter((i) => i.range)
        .sort((a, b) => a.n.rssi - b.n.rssi), // strongest drawn last (on top)
    [networks, band],
  );
  const anySelected = selected.size > 0;
  // Label one network per frequency block to keep the graph legible: the connected
  // one if present, otherwise the strongest.
  const labelled = useMemo(() => {
    const best = new Map<string, { bssid: string; score: number }>();
    for (const { n, range } of items) {
      const score = n.rssi + (n.connected ? 1000 : 0);
      const key = `${range[0]}`;
      if (!best.has(key) || best.get(key)!.score < score) best.set(key, { bssid: n.bssid, score });
    }
    return new Set(Array.from(best.values()).map((b) => b.bssid));
  }, [items]);

  return (
    <div ref={host}>
    <svg width={W} height={H} role="img" aria-label={`${band} GHz channels`} style={{ display: "block" }}>
      {[-30, -50, -70, -90].map((d) => (
        <g key={d}>
          <line x1={pad.l} x2={W - pad.r} y1={y(d)} y2={y(d)} stroke="var(--rule)" />
          <text x={pad.l - 6} y={y(d) + 4} textAnchor="end" fontSize="11" fill="var(--ink-3)">
            {d}
          </text>
        </g>
      ))}
      <line x1={pad.l} x2={W - pad.r} y1={y(threshold)} y2={y(threshold)} stroke="var(--sig-good)" strokeDasharray="4 4" opacity={0.8} />
      {axis.ticks.map((tk) => (
        <g key={tk.label}>
          <line x1={x(tk.f)} x2={x(tk.f)} y1={H - pad.b} y2={H - pad.b + 4} stroke="var(--rule-strong)" />
          <text x={x(tk.f)} y={H - pad.b + 17} textAnchor="middle" fontSize="11" fill="var(--ink-3)">
            {tk.label}
          </text>
        </g>
      ))}
      <line x1={pad.l} x2={W - pad.r} y1={H - pad.b} y2={H - pad.b} stroke="var(--rule-strong)" />
      {items.map(({ n, range }) => {
        const color = seriesColor(n.ssid || n.bssid);
        const isSel = selected.has(n.bssid);
        const faded = anySelected && !isSel;
        const x0 = x(range[0]);
        const x1 = x(range[1]);
        const yt = y(Math.max(BOTTOM, Math.min(TOP, n.rssi)));
        const yb = y(BOTTOM);
        const shoulder = Math.min(10, (x1 - x0) * 0.18);
        const d = `M${x0},${yb} C${x0 + shoulder * 0.4},${yb} ${x0 + shoulder * 0.6},${yt} ${x0 + shoulder},${yt} L${x1 - shoulder},${yt} C${x1 - shoulder * 0.6},${yt} ${x1 - shoulder * 0.4},${yb} ${x1},${yb}`;
        return (
          <g key={n.bssid} opacity={faded ? 0.18 : 1} onClick={() => onSelect(n.bssid)} style={{ cursor: "pointer" }}>
            <path d={d} fill={color} fillOpacity={isSel ? 0.22 : 0.08} stroke={color} strokeWidth={n.connected || isSel ? 2.4 : 1.3} />
            {(labelled.has(n.bssid) || isSel || n.connected) && (
              <text x={(x0 + x1) / 2} y={yt - 5} textAnchor="middle" fontSize="11.5" fontWeight={n.connected ? 700 : 500} fill={color}>
                {displaySsid(n, hiddenLabel)}
              </text>
            )}
          </g>
        );
      })}
    </svg>
    </div>
  );
}
