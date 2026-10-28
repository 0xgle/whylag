import { useMemo, useState } from "react";
import { Pause, Play, Search } from "lucide-react";
import { useApp } from "../../store";
import { useT } from "../../i18n";
import type { Band } from "../../types";
import { Panel, Segmented } from "../../components/ui";
import { ScanNotices } from "../../components/ScanNotices";
import { TimeChart, type Series } from "../../components/TimeChart";
import { displaySsid, seriesColor } from "../../lib/signal";
import { ChannelGraph } from "./ChannelGraph";
import { NetworkTable } from "./NetworkTable";
import { BssDrawer } from "./BssDrawer";
import { RoamingPanel } from "./RoamingPanel";

type BandFilter = "all" | Band;

export function WifiModule() {
  const t = useT();
  const scan = useApp((s) => s.scan);
  const history = useApp((s) => s.history);
  const paused = useApp((s) => s.scanPaused);
  const setPaused = useApp((s) => s.setScanPaused);
  const threshold = useApp((s) => s.settings?.coverageThresholdDbm ?? -67);

  const [band, setBand] = useState<BandFilter>("all");
  const [graphBand, setGraphBand] = useState<Band>("2.4");
  const [query, setQuery] = useState("");
  const [grouped, setGrouped] = useState(true);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [open, setOpen] = useState<string | null>(null);

  const networks = scan?.networks ?? [];
  const bandsPresent = useMemo(() => new Set(networks.map((n) => n.band).filter(Boolean) as Band[]), [networks]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return networks.filter(
      (n) =>
        (band === "all" || n.band === band) &&
        (!q || n.ssid.toLowerCase().includes(q) || n.bssid.includes(q) || (n.vendor ?? "").toLowerCase().includes(q)),
    );
  }, [networks, band, query]);

  const effectiveGraphBand: Band = band === "all" ? graphBand : band;

  const toggle = (bssid: string) =>
    setSelected((s) => {
      const next = new Set(s);
      if (next.has(bssid)) next.delete(bssid);
      else next.add(bssid);
      return next;
    });

  // Chart the selection, or the connected radio plus the strongest networks.
  const charted = useMemo(() => {
    const pick = selected.size
      ? networks.filter((n) => selected.has(n.bssid))
      : [...networks.filter((n) => n.connected), ...filtered.filter((n) => !n.connected).slice(0, 4)];
    return pick.slice(0, 10);
  }, [selected, networks, filtered]);

  const series: Series[] = useMemo(
    () =>
      charted.map((n) => ({
        label: `${displaySsid(n, t("common.hidden"))} (${n.channel})`,
        color: seriesColor(n.ssid + n.bssid),
        width: n.connected ? 2.5 : 1.6,
        points: (history[n.bssid] ?? []).map((p) => [p.t, p.rssi] as [number, number]),
      })),
    [charted, history, t],
  );

  const ssidCount = new Set(networks.map((n) => (n.hidden ? n.bssid : n.ssid))).size;
  const openBss = open ? networks.find((n) => n.bssid === open) : null;

  return (
    <div className="stack">
      <ScanNotices />
      <div className="row">
        <Segmented<BandFilter>
          label="Band"
          value={band}
          onChange={setBand}
          options={[
            { value: "all", label: t("common.all") },
            { value: "2.4", label: "2.4 GHz" },
            { value: "5", label: "5 GHz" },
            ...(bandsPresent.has("6") ? [{ value: "6" as const, label: "6 GHz" }] : []),
          ]}
        />
        <label className="row" style={{ gap: 6, position: "relative" }}>
          <Search size={15} style={{ position: "absolute", left: 9, color: "var(--ink-3)" }} aria-hidden />
          <input
            className="input"
            style={{ paddingLeft: 30, width: 220 }}
            placeholder={t("common.search")}
            aria-label={t("common.search")}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </label>
        <label className="check">
          <input type="checkbox" checked={grouped} onChange={(e) => setGrouped(e.target.checked)} />
          {t("wifi.groupBySsid")}
        </label>
        <span className="spacer" />
        <span className="muted small">
          {t("wifi.networks", { n: networks.length })}, {t("wifi.ssids", { n: ssidCount })}
        </span>
        <button className="btn" onClick={() => setPaused(!paused)}>
          {paused ? <Play size={15} /> : <Pause size={15} />}
          {paused ? t("common.resume") : t("common.pause")}
        </button>
      </div>

      <div className="grid-2">
        <Panel
          title={t("wifi.channels")}
          actions={
            band === "all" ? (
              <Segmented<Band>
                label="Graph band"
                value={graphBand}
                onChange={setGraphBand}
                options={(["2.4", "5", "6"] as Band[]).filter((b) => b !== "6" || bandsPresent.has("6")).map((b) => ({ value: b, label: `${b} GHz` }))}
              />
            ) : undefined
          }
        >
          <ChannelGraph
            band={effectiveGraphBand}
            networks={filtered}
            selected={selected}
            onSelect={toggle}
            hiddenLabel={t("common.hidden")}
            threshold={threshold}
          />
        </Panel>
        <Panel title={t("wifi.signalHistory")} actions={!selected.size ? <span className="muted small">{t("wifi.selectHint")}</span> : undefined}>
          <TimeChart series={series} height={232} yRange={[-95, -25]} yLabel="dBm" ariaLabel={t("wifi.signalHistory")} />
        </Panel>
      </div>

      <Panel bodyClass="">
        <NetworkTable networks={filtered} selected={selected} onToggle={toggle} onOpen={setOpen} grouped={grouped} />
      </Panel>

      <RoamingPanel />

      {openBss && <BssDrawer bss={openBss} all={networks} onClose={() => setOpen(null)} />}
    </div>
  );
}
