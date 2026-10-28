import { Fragment, useMemo, useState } from "react";
import { ChevronDown, ChevronRight, Link2 } from "lucide-react";
import type { Bss } from "../../types";
import { useT, type Key } from "../../i18n";
import { Badge, SignalBar } from "../../components/ui";
import { displaySsid, securityTone } from "../../lib/signal";

type SortKey = "ssid" | "rssi" | "channel" | "width" | "gen" | "security" | "vendor" | "clients" | "load";

function sortValue(n: Bss, key: SortKey): number | string {
  switch (key) {
    case "ssid":
      return (n.ssid || "￿").toLowerCase();
    case "rssi":
      return n.rssi;
    case "channel":
      return n.frequencyMhz;
    case "width":
      return n.widthMhz;
    case "gen":
      return n.wifiGen ?? 0;
    case "security":
      return n.security;
    case "vendor":
      return (n.vendor ?? "￿").toLowerCase();
    case "clients":
      return n.stationCount ?? -1;
    case "load":
      return n.utilizationPct ?? -1;
  }
}

export function NetworkTable({
  networks,
  selected,
  onToggle,
  onOpen,
  grouped,
}: {
  networks: Bss[];
  selected: Set<string>;
  onToggle: (bssid: string) => void;
  onOpen: (bssid: string) => void;
  grouped: boolean;
}) {
  const t = useT();
  const [sort, setSort] = useState<{ key: SortKey; dir: 1 | -1 }>({ key: "rssi", dir: -1 });
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());

  const sorted = useMemo(() => {
    const list = networks.slice();
    list.sort((a, b) => {
      const va = sortValue(a, sort.key);
      const vb = sortValue(b, sort.key);
      const c = va < vb ? -1 : va > vb ? 1 : 0;
      return c * sort.dir || b.rssi - a.rssi;
    });
    return list;
  }, [networks, sort]);

  const groups = useMemo(() => {
    if (!grouped) return null;
    const map = new Map<string, Bss[]>();
    for (const n of sorted) {
      const k = n.hidden ? `hidden:${n.bssid}` : n.ssid;
      map.set(k, [...(map.get(k) ?? []), n]);
    }
    // The network you are connected to always leads.
    return Array.from(map.entries()).sort(([, a], [, b]) => Number(b.some((n) => n.connected)) - Number(a.some((n) => n.connected)));
  }, [sorted, grouped]);

  const header = (key: SortKey, label: Key, num = false) => (
    <th
      data-sortable="true"
      className={num ? "num" : undefined}
      aria-sort={sort.key === key ? (sort.dir === 1 ? "ascending" : "descending") : undefined}
      onClick={() => setSort((s) => ({ key, dir: s.key === key ? ((-s.dir) as 1 | -1) : key === "ssid" || key === "vendor" ? 1 : -1 }))}
    >
      {t(label)}
      {sort.key === key ? (sort.dir === 1 ? " ▲" : " ▼") : ""}
    </th>
  );

  const row = (n: Bss, indent = false) => (
    <tr key={n.bssid} aria-selected={selected.has(n.bssid)} onClick={() => onToggle(n.bssid)} onDoubleClick={() => onOpen(n.bssid)}>
      <td style={{ paddingLeft: indent ? 34 : undefined }}>
        <span className="row" style={{ gap: 6, flexWrap: "nowrap" }}>
          {n.connected && <Link2 size={14} aria-label={t("wifi.connected")} color="var(--accent)" />}
          <span className={n.connected ? "cell-strong" : undefined}>{displaySsid(n, t("common.hidden"))}</span>
        </span>
      </td>
      <td className="mono-ish">{n.bssid}</td>
      <td>{n.vendor ?? <span className="muted">—</span>}</td>
      <td>
        <SignalBar rssi={n.rssi} />
      </td>
      <td className="num">
        {n.channel}
        <span className="muted small"> {n.band} GHz</span>
      </td>
      <td className="num">{n.widthMhz}</td>
      <td className="num">{n.wifiGen ?? <span className="muted">—</span>}</td>
      <td>
        <span className="row" style={{ gap: 4, flexWrap: "nowrap" }}>
          <Badge tone={securityTone(n.security)}>{t(`security.${n.security}` as Key)}</Badge>
          {n.wps && <Badge tone="warn">WPS</Badge>}
        </span>
      </td>
      <td className="num">{n.stationCount ?? <span className="muted">—</span>}</td>
      <td className="num">{n.utilizationPct != null ? `${n.utilizationPct}%` : <span className="muted">—</span>}</td>
      <td className="mono-ish">
        {n.roaming.k ? "k" : "–"}
        {n.roaming.v ? "v" : "–"}
        {n.roaming.r ? "r" : "–"}
      </td>
    </tr>
  );

  return (
    <div className="table-wrap" style={{ maxHeight: 520 }}>
      <table className="data">
        <thead>
          <tr>
            {header("ssid", "wifi.col.ssid")}
            <th>{t("wifi.col.bssid")}</th>
            {header("vendor", "wifi.col.vendor")}
            {header("rssi", "wifi.col.signal")}
            {header("channel", "wifi.col.channel", true)}
            {header("width", "wifi.col.width", true)}
            {header("gen", "wifi.col.gen", true)}
            {header("security", "wifi.col.security")}
            {header("clients", "wifi.col.clients", true)}
            {header("load", "wifi.col.load", true)}
            <th title="802.11k / 802.11v / 802.11r">{t("wifi.col.roaming")}</th>
          </tr>
        </thead>
        <tbody>
          {groups
            ? groups.map(([key, list]) => {
                const best = list[0];
                const isCollapsed = collapsed.has(key);
                if (list.length === 1) return row(best);
                return (
                  <Fragment key={key}>
                    <tr
                      onClick={() =>
                        setCollapsed((c) => {
                          const next = new Set(c);
                          if (next.has(key)) next.delete(key);
                          else next.add(key);
                          return next;
                        })
                      }
                    >
                      <td colSpan={3}>
                        <span className="row" style={{ gap: 6, flexWrap: "nowrap" }}>
                          {isCollapsed ? <ChevronRight size={15} /> : <ChevronDown size={15} />}
                          <span className="cell-strong">{displaySsid(best, t("common.hidden"))}</span>
                          <span className="muted small">{t("wifi.radios", { n: list.length })}</span>
                          {list.some((n) => n.connected) && <Link2 size={14} color="var(--accent)" />}
                        </span>
                      </td>
                      <td>
                        <SignalBar rssi={best.rssi} />
                      </td>
                      <td colSpan={7} className="muted small">
                        {Array.from(new Set(list.map((n) => `${n.channel}`))).join(", ")}
                      </td>
                    </tr>
                    {!isCollapsed && list.map((n) => row(n, true))}
                  </Fragment>
                );
              })
            : sorted.map((n) => row(n))}
        </tbody>
      </table>
      {networks.length === 0 && <div className="empty">{t("wifi.empty")}</div>}
    </div>
  );
}
