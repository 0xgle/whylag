import type { Band, Bss, Security } from "../types";

export type Quality = "excellent" | "good" | "fair" | "poor" | "unusable";

/** Buckets a Wi-Fi signal level. Thresholds follow common enterprise design targets. */
export function quality(rssi: number): Quality {
  if (rssi >= -55) return "excellent";
  if (rssi >= -67) return "good";
  if (rssi >= -75) return "fair";
  if (rssi >= -82) return "poor";
  return "unusable";
}

export const qualityColor: Record<Quality, string> = {
  excellent: "var(--sig-excellent)",
  good: "var(--sig-good)",
  fair: "var(--sig-fair)",
  poor: "var(--sig-poor)",
  unusable: "var(--sig-bad)",
};

/** 0..1 position of an RSSI between -95 and -30 dBm. */
export function rssiFraction(rssi: number) {
  return Math.min(1, Math.max(0, (rssi + 95) / 65));
}

export function securityTone(s: Security): "crit" | "warn" | "ok" | undefined {
  if (s === "open" || s === "wep") return "crit";
  if (s === "wpa" || s === "wpaEnterprise") return "warn";
  if (s === "wpa3" || s === "wpa3Enterprise" || s === "wpa2Wpa3" || s === "owe") return "ok";
  return undefined;
}

export function channelToFreq(channel: number, band: Band): number {
  if (band === "2.4") return channel === 14 ? 2484 : 2407 + channel * 5;
  if (band === "5") return 5000 + channel * 5;
  return 5950 + channel * 5;
}

/** Frequency block (MHz) a BSS occupies; mirrors `rf::occupied_range` in the core. */
export function occupiedRange(channel: number, band: Band, width: number): [number, number] {
  const primary = channelToFreq(channel, band);
  const w = Math.max(20, width);
  if (band === "2.4") {
    if (width >= 40) return channel <= 7 ? [primary - 10, primary + 30] : [primary - 30, primary + 10];
    return [primary - 11, primary + 11];
  }
  const origin = band === "5" ? (primary >= 5745 ? 5735 : 5170) : 5945;
  const start = origin + Math.floor((primary - 10 - origin) / w) * w;
  return [start, start + w];
}

export function overlaps(a: [number, number], b: [number, number]) {
  return a[0] < b[1] && b[0] < a[1];
}

export function bssRange(b: Bss): [number, number] | null {
  if (!b.band) return null;
  return occupiedRange(b.channel, b.band, b.widthMhz);
}

export function displaySsid(b: Pick<Bss, "ssid" | "hidden">, hiddenLabel: string) {
  return b.hidden || !b.ssid ? hiddenLabel : b.ssid;
}

/** Stable, distinguishable colour per network for charts. */
const PALETTE = ["#2433c9", "#d9632b", "#12805a", "#9b3fb5", "#c99a12", "#1c8fb0", "#c0263d", "#5d6b2f", "#7a4b2a", "#4a5563"];
export function seriesColor(key: string) {
  let h = 0;
  for (let i = 0; i < key.length; i++) h = (h * 31 + key.charCodeAt(i)) >>> 0;
  return PALETTE[h % PALETTE.length];
}
