import { useMemo } from "react";
import { useApp } from "../store";
import { useT } from "../i18n";
import { quality, qualityColor } from "../lib/signal";
import { Sparkline } from "./ui";
import type { Key } from "../i18n";

/** The instrument display in the header: live link signal, always visible. */
export function LinkReadout() {
  const t = useT();
  const scan = useApp((s) => s.scan);
  const history = useApp((s) => s.history);
  const iface = scan?.interfaces.find((i) => i.state === "connected") ?? scan?.interfaces[0];
  const bss = scan?.networks.find((n) => n.connected);
  const rssi = iface?.rssi ?? bss?.rssi ?? null;
  const spark = useMemo(() => (bss ? (history[bss.bssid] ?? []).slice(-40).map((p) => p.rssi) : []), [bss, history]);
  const sim = scan?.source === "simulator";

  const sourceChip = scan && (
    <span className="source-chip" data-sim={sim}>
      <span className="dot" aria-hidden />
      {sim ? t("source.simulator") : t(`source.${scan.source}` as Key)}
    </span>
  );

  if (!scan) return <div className="readout muted small">{t("common.loading")}</div>;

  if (!iface || iface.state !== "connected" || rssi === null) {
    return (
      <div className="readout">
        <div className="readout-meta">
          <div className="readout-ssid muted">{t("link.notConnected")}</div>
          <div className="readout-sub">{sourceChip}</div>
        </div>
      </div>
    );
  }

  const q = quality(rssi);
  const snr = bss?.noise != null ? rssi - bss.noise : null;
  const band = iface.band ?? bss?.band;
  const sub = [
    iface.channel != null ? t("link.channel", { ch: iface.channel }) + (band ? ` (${band} GHz)` : "") : null,
    snr != null ? t("link.snr", { snr }) : null,
    iface.txRateMbps != null ? t("link.rate", { rate: Math.round(iface.txRateMbps) }) : null,
  ].filter(Boolean) as string[];

  return (
    <div className="readout" aria-label={t("link.signal")}>
      <div className="readout-meta">
        <div className="readout-ssid" title={iface.bssid ?? undefined}>
          {iface.ssid}
        </div>
        <div className="readout-sub readout-facts">
          {sub.map((s) => (
            <span key={s}>{s}</span>
          ))}
        </div>
        <div className="readout-sub" style={{ marginTop: 3 }}>
          {sourceChip}
        </div>
      </div>
      <Sparkline values={spark} color={qualityColor[q]} width={92} height={34} />
      <div>
        <div className="readout-value" style={{ color: qualityColor[q] }}>
          <span className="readout-number">{rssi}</span>
          <span className="readout-unit">dBm</span>
        </div>
        <div className="readout-sub" style={{ textAlign: "right", marginTop: 4 }}>
          {t(`quality.${q}` as Key)}
        </div>
      </div>
    </div>
  );
}
