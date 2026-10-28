import type { Layer } from "../../types";

type Stop = [number, [number, number, number]];

// Colour scales tuned so the coverage target sits where yellow turns green.
// Values below the first stop clamp to its colour.
const SCALES: Record<Exclude<Layer, "bestAp">, Stop[]> = {
  signal: [
    [-90, [150, 25, 45]],
    [-80, [214, 72, 40]],
    [-72, [240, 160, 40]],
    [-67, [235, 214, 60]],
    [-60, [120, 196, 80]],
    [-50, [40, 160, 95]],
    [-40, [20, 120, 140]],
  ],
  secondSignal: [
    [-90, [150, 25, 45]],
    [-80, [214, 72, 40]],
    [-72, [240, 160, 40]],
    [-67, [235, 214, 60]],
    [-60, [120, 196, 80]],
    [-50, [40, 160, 95]],
    [-40, [20, 120, 140]],
  ],
  snr: [
    [0, [150, 25, 45]],
    [10, [214, 72, 40]],
    [20, [240, 180, 40]],
    [25, [190, 210, 70]],
    [32, [60, 170, 90]],
    [45, [20, 120, 140]],
  ],
  apCount: [
    [0, [150, 25, 45]],
    [1, [240, 160, 40]],
    [2, [80, 180, 90]],
    [3, [20, 120, 140]],
  ],
  coChannel: [
    [0, [40, 160, 95]],
    [1, [190, 210, 70]],
    [2, [240, 160, 40]],
    [3, [214, 72, 40]],
    [5, [150, 25, 45]],
  ],
  throughput: [
    [0, [150, 25, 45]],
    [20, [214, 72, 40]],
    [50, [240, 180, 40]],
    [150, [110, 190, 80]],
    [400, [20, 120, 140]],
  ],
};

const CATEGORICAL: [number, number, number][] = [
  [36, 51, 201],
  [217, 99, 43],
  [18, 128, 90],
  [155, 63, 181],
  [201, 154, 18],
  [28, 143, 176],
  [192, 38, 61],
  [93, 107, 47],
  [122, 75, 42],
  [74, 85, 99],
  [214, 110, 160],
  [70, 160, 200],
];

export function colorFor(layer: Layer, v: number): [number, number, number] {
  if (layer === "bestAp") return CATEGORICAL[Math.abs(Math.round(v)) % CATEGORICAL.length];
  const stops = SCALES[layer];
  if (v <= stops[0][0]) return stops[0][1];
  for (let i = 1; i < stops.length; i++) {
    if (v <= stops[i][0]) {
      const [v0, c0] = stops[i - 1];
      const [v1, c1] = stops[i];
      const f = (v - v0) / (v1 - v0);
      return [c0[0] + (c1[0] - c0[0]) * f, c0[1] + (c1[1] - c0[1]) * f, c0[2] + (c1[2] - c0[2]) * f];
    }
  }
  return stops[stops.length - 1][1];
}

export function cssColor(c: [number, number, number]) {
  return `rgb(${Math.round(c[0])}, ${Math.round(c[1])}, ${Math.round(c[2])})`;
}

/** Legend ticks for continuous layers. */
export function legendStops(layer: Exclude<Layer, "bestAp">) {
  return SCALES[layer].map(([v, c]) => ({ v, color: cssColor(c) }));
}

export function unitFor(layer: Layer) {
  switch (layer) {
    case "signal":
    case "secondSignal":
      return "dBm";
    case "snr":
      return "dB";
    case "throughput":
      return "Mb/s";
    default:
      return "";
  }
}
