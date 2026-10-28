import { Cloud } from "lucide-react";
import type { Host, Link } from "../../types";
import { useT, type Key } from "../../i18n";
import { DEVICE_ICON, KIND_ORDER } from "../../lib/deviceIcons";
import { useWidth } from "../../lib/useWidth";

function label(h: Host, unnamed: string) {
  return h.name ?? h.model ?? (h.vendor ? `${h.vendor}` : unnamed);
}

function Node({ x, y, host, onOpen, unnamed }: { x: number; y: number; host: Host; onOpen: (ip: string) => void; unnamed: string }) {
  const Icon = DEVICE_ICON[host.kind];
  const name = label(host, unnamed);
  return (
    <g transform={`translate(${x} ${y})`} className="topo-node" onClick={() => onOpen(host.ip)} role="button" tabIndex={0} onKeyDown={(e) => e.key === "Enter" && onOpen(host.ip)}>
      <rect x={-62} y={-24} width={124} height={48} rx={8} />
      <foreignObject x={-56} y={-17} width={22} height={22}>
        <Icon size={20} />
      </foreignObject>
      <text x={-30} y={-3} className="topo-name">
        {name.length > 15 ? name.slice(0, 14) + "…" : name}
      </text>
      <text x={-30} y={13} className="topo-ip">
        {host.ip}
      </text>
    </g>
  );
}

/**
 * Network map: internet, gateway, switches and their LLDP-reported links as a
 * tree; every other device grouped by type underneath.
 */
export function TopologyMap({ hosts, links, onOpen }: { hosts: Host[]; links: Link[]; onOpen: (ip: string) => void }) {
  const t = useT();
  const [host, width] = useWidth<HTMLDivElement>(900);
  const unnamed = t("discovery.unnamed");
  const gateway = hosts.find((h) => h.isGateway);
  const switches = hosts.filter((h) => h.kind === "switch" || links.some((l) => l.from === h.ip));
  const linkedIps = new Set(links.map((l) => l.neighborIp).filter(Boolean) as string[]);
  const children = (sw: Host) =>
    links
      .filter((l) => l.from === sw.ip && l.neighborIp && l.neighborIp !== gateway?.ip)
      .map((l) => ({ link: l, host: hosts.find((h) => h.ip === l.neighborIp) }))
      .filter((c): c is { link: Link; host: Host } => !!c.host);
  const placed = new Set<string>([gateway?.ip, ...switches.map((s) => s.ip), ...Array.from(linkedIps)].filter(Boolean) as string[]);
  const rest = hosts.filter((h) => !placed.has(h.ip));
  const groups = KIND_ORDER.map((k) => ({ kind: k, list: rest.filter((h) => h.kind === k) })).filter((g) => g.list.length);

  const W = Math.max(width, 640);
  const rowY = [40, 130, 230, 340];
  const cx = W / 2;
  const leafRows = switches.map(children);
  const leaves = leafRows.flat();
  const leafSpacing = Math.min(150, (W - 40) / Math.max(1, leaves.length));
  const H = leaves.length ? rowY[3] + 40 : switches.length ? rowY[2] + 40 : rowY[1] + 40;
  let leafIndex = 0;
  const leafStart = cx - ((leaves.length - 1) * leafSpacing) / 2;
  const swSpacing = Math.min(220, (W - 40) / Math.max(1, switches.length));
  const swStart = cx - ((switches.length - 1) * swSpacing) / 2;

  return (
    <div ref={host} className="topo">
      <svg width={W} height={H} role="img" aria-label="Network map">
        <g className="topo-lines">
          {gateway && <line x1={cx} y1={rowY[0] + 18} x2={cx} y2={rowY[1] - 24} />}
          {switches.map((sw, i) => {
            const x = swStart + i * swSpacing;
            return <line key={sw.ip} x1={cx} y1={rowY[1] + 24} x2={x} y2={rowY[2] - 24} />;
          })}
          {switches.map((sw, i) => {
            const x = swStart + i * swSpacing;
            return children(sw).map((c) => {
              const lx = leafStart + leafIndex++ * leafSpacing;
              return (
                <g key={c.host.ip}>
                  <line x1={x} y1={rowY[2] + 24} x2={lx} y2={rowY[3] - 24} />
                  {c.link.localPort && (
                    <text x={(x + lx) / 2} y={(rowY[2] + rowY[3]) / 2} className="topo-port">
                      {c.link.localPort}
                    </text>
                  )}
                </g>
              );
            });
          })}
        </g>
        <g transform={`translate(${cx} ${rowY[0]})`} className="topo-cloud">
          <foreignObject x={-14} y={-16} width={28} height={28}>
            <Cloud size={26} />
          </foreignObject>
          <text y={26} textAnchor="middle">
            {t("discovery.map.internet")}
          </text>
        </g>
        {gateway && <Node x={cx} y={rowY[1]} host={gateway} onOpen={onOpen} unnamed={unnamed} />}
        {switches.map((sw, i) => (
          <Node key={sw.ip} x={swStart + i * swSpacing} y={rowY[2]} host={sw} onOpen={onOpen} unnamed={unnamed} />
        ))}
        {(() => {
          let i = 0;
          return switches.flatMap((sw) => children(sw).map((c) => <Node key={c.host.ip} x={leafStart + i++ * leafSpacing} y={rowY[3]} host={c.host} onOpen={onOpen} unnamed={unnamed} />));
        })()}
      </svg>
      {links.length > 0 && <p className="small muted" style={{ margin: "4px 0 16px" }}>{t("discovery.map.lldp")}</p>}
      <div className="topo-groups">
        <h3 className="topo-groups-title">{t("discovery.map.wireless")}</h3>
        {groups.map((g) => {
          const Icon = DEVICE_ICON[g.kind];
          return (
            <div key={g.kind} className="topo-group">
              <div className="topo-group-head">
                <Icon size={15} />
                {t(`kind.${g.kind}` as Key)}
                <span className="muted">{g.list.length}</span>
              </div>
              <div className="topo-cards">
                {g.list.map((h) => (
                  <button key={h.ip} className="topo-card" onClick={() => onOpen(h.ip)}>
                    <span className="topo-card-name">{label(h, unnamed)}</span>
                    <span className="topo-card-ip">{h.ip}</span>
                  </button>
                ))}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
