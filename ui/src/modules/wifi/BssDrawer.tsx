import { X } from "lucide-react";
import type { Bss } from "../../types";
import { useT, type Key } from "../../i18n";
import { Badge, SignalBar } from "../../components/ui";
import { bssRange, displaySsid, overlaps, securityTone } from "../../lib/signal";

export function BssDrawer({ bss, all, onClose }: { bss: Bss; all: Bss[]; onClose: () => void }) {
  const t = useT();
  const range = bssRange(bss);
  const sharing = range
    ? all.filter((o) => o.bssid !== bss.bssid && o.band === bss.band && bssRange(o) && overlaps(range, bssRange(o)!)).sort((a, b) => b.rssi - a.rssi)
    : [];
  const snr = bss.noise != null ? bss.rssi - bss.noise : null;
  const roam = [bss.roaming.k && "802.11k", bss.roaming.v && "802.11v", bss.roaming.r && "802.11r"].filter(Boolean).join(", ");

  return (
    <aside className="drawer" aria-label={t("wifi.detail.title")}>
      <header className="panel-head">
        <div>
          <h2 className="panel-title">{displaySsid(bss, t("common.hidden"))}</h2>
          <div className="small muted mono-ish">{bss.bssid}</div>
        </div>
        <button className="btn btn-ghost btn-sm" onClick={onClose} aria-label={t("common.close")}>
          <X size={16} />
        </button>
      </header>
      <div className="drawer-body stack">
        <div className="row">
          <SignalBar rssi={bss.rssi} width={120} />
          <span className="muted">dBm</span>
          <span className="spacer" />
          <Badge tone={securityTone(bss.security)}>{t(`security.${bss.security}` as Key)}</Badge>
        </div>
        {bss.weakCipher && <Badge tone="crit">{t("wifi.detail.weakCipher")}</Badge>}
        <dl className="kv">
          <dt>{t("wifi.col.vendor")}</dt>
          <dd>{bss.vendor ?? "—"}</dd>
          <dt>{t("wifi.col.channel")}</dt>
          <dd>
            {bss.channel} ({bss.band} GHz), {bss.widthMhz} MHz
          </dd>
          <dt>{t("wifi.detail.frequency")}</dt>
          <dd>{range ? `${range[0]}–${range[1]} MHz` : `${bss.frequencyMhz} MHz`}</dd>
          <dt>{t("wifi.col.gen")}</dt>
          <dd>{bss.wifiGen ? `Wi-Fi ${bss.wifiGen}` : "802.11a/b/g"}</dd>
          {bss.noise != null && (
            <>
              <dt>{t("wifi.detail.noise")}</dt>
              <dd>{bss.noise} dBm</dd>
              <dt>{t("wifi.detail.snr")}</dt>
              <dd>{snr} dB</dd>
            </>
          )}
          <dt>{t("wifi.detail.pmf")}</dt>
          <dd>{t(`wifi.pmf.${bss.pmf}` as Key)}</dd>
          <dt>{t("wifi.detail.wps")}</dt>
          <dd>{bss.wps ? t("common.yes") : t("common.no")}</dd>
          <dt>{t("wifi.detail.roaming")}</dt>
          <dd>{roam || t("common.none")}</dd>
          {bss.stationCount != null && (
            <>
              <dt>{t("wifi.col.clients")}</dt>
              <dd>{bss.stationCount}</dd>
            </>
          )}
          {bss.utilizationPct != null && (
            <>
              <dt>{t("wifi.col.load")}</dt>
              <dd>{bss.utilizationPct}%</dd>
            </>
          )}
          {bss.country && (
            <>
              <dt>{t("wifi.detail.country")}</dt>
              <dd>{bss.country}</dd>
            </>
          )}
          {bss.beaconInterval != null && (
            <>
              <dt>{t("wifi.detail.beacon")}</dt>
              <dd>{bss.beaconInterval} TU</dd>
            </>
          )}
        </dl>
        <div>
          <div className="panel-title" style={{ fontSize: 14, marginBottom: 8 }}>
            {t("wifi.detail.overlaps")}
          </div>
          {sharing.length === 0 ? (
            <div className="muted small">{t("wifi.detail.overlapsNone")}</div>
          ) : (
            <table className="data">
              <tbody>
                {sharing.map((o) => (
                  <tr key={o.bssid} style={{ cursor: "default" }}>
                    <td>{displaySsid(o, t("common.hidden"))}</td>
                    <td className="num">
                      {o.channel} / {o.widthMhz}
                    </td>
                    <td>
                      <SignalBar rssi={o.rssi} width={48} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </div>
    </aside>
  );
}
