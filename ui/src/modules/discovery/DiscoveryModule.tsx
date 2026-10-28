import { useMemo, useState } from "react";
import { ExternalLink, Network, Search, Square, X } from "lucide-react";
import { useT, useLang, translateMaybe, fmtNumber, type Key } from "../../i18n";
import { openExternal } from "../../api";
import type { DeviceKind, DiscoveryRun, Host } from "../../types";
import { useJob } from "../../lib/useJob";
import { DEVICE_ICON, KIND_ORDER } from "../../lib/deviceIcons";
import { Badge, Intro, Notice, Panel, Progress, Segmented } from "../../components/ui";
import { TopologyMap } from "./TopologyMap";

function HostDrawer({ host, onClose }: { host: Host; onClose: () => void }) {
  const t = useT();
  const lang = useLang();
  const Icon = DEVICE_ICON[host.kind];
  const web = host.ports.find((p) => [80, 443, 8080, 8443, 5000, 5001].includes(p.port));
  const webUrl = web ? `${[443, 8443, 5001].includes(web.port) ? "https" : "http"}://${host.ip}${[80, 443].includes(web.port) ? "" : `:${web.port}`}/` : null;
  return (
    <aside className="drawer" aria-label={host.name ?? host.ip}>
      <header className="panel-head">
        <div className="row" style={{ gap: 10, flexWrap: "nowrap" }}>
          <span className="device-icon">
            <Icon size={20} />
          </span>
          <div>
            <h2 className="panel-title">{host.name ?? t("discovery.unnamed")}</h2>
            <div className="small muted">
              {t(`kind.${host.kind}` as Key)}
              {host.os ? `, ${host.os}` : ""}
            </div>
          </div>
        </div>
        <button className="btn btn-ghost btn-sm" onClick={onClose} aria-label={t("common.close")}>
          <X size={16} />
        </button>
      </header>
      <div className="drawer-body stack">
        <dl className="kv">
          <dt>{t("discovery.col.ip")}</dt>
          <dd>{host.ip}</dd>
          <dt>MAC</dt>
          <dd className="mono-ish">
            {host.mac ?? "—"} {host.randomMac && <Badge>{t("discovery.privateMac")}</Badge>}
          </dd>
          <dt>{t("wifi.col.vendor")}</dt>
          <dd>{host.vendor ?? "—"}</dd>
          {host.model && (
            <>
              <dt>{t("discovery.detail.model")}</dt>
              <dd>{host.model}</dd>
            </>
          )}
          {host.rttMs != null && (
            <>
              <dt>{t("discovery.col.rtt")}</dt>
              <dd>{fmtNumber(lang, host.rttMs, 1)} ms</dd>
            </>
          )}
          {host.snmpDescr && (
            <>
              <dt>{t("discovery.detail.snmp")}</dt>
              <dd>{host.snmpDescr}</dd>
            </>
          )}
          {(host.httpTitle || host.httpServer) && (
            <>
              <dt>{t("discovery.detail.web")}</dt>
              <dd>{[host.httpTitle, host.httpServer].filter(Boolean).join(", ")}</dd>
            </>
          )}
        </dl>
        {webUrl && (
          <button className="btn" onClick={() => openExternal(webUrl)} style={{ alignSelf: "flex-start" }}>
            <ExternalLink size={15} />
            {t("discovery.detail.openWeb")}
          </button>
        )}
        {host.evidence.length > 0 && (
          <div>
            <div className="drawer-sub">{t("discovery.detail.evidence")}</div>
            <ul className="plain-list">
              {host.evidence.map((e) => (
                <li key={e}>{translateMaybe(lang, `evidence.${e}`, e)}</li>
              ))}
            </ul>
          </div>
        )}
        {host.ports.length > 0 && (
          <div>
            <div className="drawer-sub">{t("discovery.col.ports")}</div>
            <div className="chips">
              {host.ports.map((p) => (
                <Badge key={p.port} tone={[23, 21, 5900].includes(p.port) ? "warn" : undefined}>
                  {p.port} {p.service}
                </Badge>
              ))}
            </div>
          </div>
        )}
        {host.mdnsServices.length > 0 && (
          <div>
            <div className="drawer-sub">{t("discovery.detail.services")}</div>
            <div className="chips">
              {host.mdnsServices.map((s) => (
                <Badge key={s}>{s}</Badge>
              ))}
            </div>
          </div>
        )}
        {host.names.length > 0 && (
          <div>
            <div className="drawer-sub">{t("discovery.detail.names")}</div>
            <dl className="kv">
              {host.names.map(([src, n], i) => (
                <div key={i} style={{ display: "contents" }}>
                  <dt>{src}</dt>
                  <dd>{n}</dd>
                </div>
              ))}
            </dl>
          </div>
        )}
      </div>
    </aside>
  );
}

export function DiscoveryModule() {
  const t = useT();
  const lang = useLang();
  const { run, start, stop, error } = useJob<DiscoveryRun>("discovery");
  const [view, setView] = useState<"list" | "map">("list");
  const [kind, setKind] = useState<DeviceKind | null>(null);
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState<string | null>(null);

  const hosts = run?.hosts ?? [];
  const counts = useMemo(() => {
    const m = new Map<DeviceKind, number>();
    for (const h of hosts) m.set(h.kind, (m.get(h.kind) ?? 0) + 1);
    return KIND_ORDER.filter((k) => m.has(k)).map((k) => ({ kind: k, n: m.get(k)! }));
  }, [hosts]);
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return hosts.filter(
      (h) =>
        (!kind || h.kind === kind) &&
        (!q || [h.name, h.ip, h.mac, h.vendor, h.model, h.os].some((v) => v?.toLowerCase().includes(q))),
    );
  }, [hosts, kind, query]);
  const openHost = hosts.find((h) => h.ip === open);
  const running = !!run?.running;
  const hasResults = hosts.length > 0;

  if (!run || (!running && !hasResults && !run.error)) {
    return (
      <Intro
        title={t("discovery.intro.title")}
        body={t("discovery.intro.body")}
        action={
          <button className="btn btn-primary" onClick={() => start()}>
            <Network size={16} />
            {t("discovery.scan")}
          </button>
        }
      />
    );
  }

  const local = run.local;
  return (
    <div className="stack">
      {error && <Notice tone="crit">{error}</Notice>}
      {run.error && <Notice tone="crit">{translateMaybe(lang, `discovery.error.${run.error}`, run.error)}</Notice>}
      <div className="toolbar">
        {running ? (
          <button className="btn" onClick={stop}>
            <Square size={14} />
            {t("common.stop")}
          </button>
        ) : (
          <button className="btn btn-primary" onClick={() => start()}>
            <Network size={16} />
            {t("discovery.rescan")}
          </button>
        )}
        {running && <Progress value={run.progress} label={translateMaybe(lang, `discovery.phase.${run.phase}`, t("discovery.scanning"))} />}
        {local && (
          <span className="small muted">
            {t("discovery.via", { iface: local.friendlyName ?? local.interfaceName, ip: local.ipv4 ?? "?", prefix: local.prefixLen ?? "?" })}
            {local.gateway ? `, ${t("discovery.gateway", { ip: local.gateway })}` : ""}
          </span>
        )}
        <span className="spacer" />
        <Segmented
          value={view}
          onChange={setView}
          options={[
            { value: "list", label: t("discovery.view.list") },
            { value: "map", label: t("discovery.view.map") },
          ]}
        />
      </div>

      <div className="kind-strip">
        <button className="kind-chip" aria-pressed={!kind} onClick={() => setKind(null)}>
          <span className="kind-chip-n">{hosts.length}</span>
          {t("common.all")}
        </button>
        {counts.map(({ kind: k, n }) => {
          const Icon = DEVICE_ICON[k];
          return (
            <button key={k} className="kind-chip" aria-pressed={kind === k} onClick={() => setKind(kind === k ? null : k)}>
              <Icon size={15} />
              <span className="kind-chip-n">{n}</span>
              {t(`kind.${k}` as Key)}
            </button>
          );
        })}
      </div>

      {view === "map" ? (
        <Panel>
          <TopologyMap hosts={hosts} links={run.links} onOpen={setOpen} />
        </Panel>
      ) : (
        <Panel
          bodyClass=""
          title={t("discovery.count", { n: filtered.length })}
          actions={
            <label className="row" style={{ gap: 6, position: "relative" }}>
              <Search size={15} style={{ position: "absolute", left: 9, color: "var(--ink-3)" }} aria-hidden />
              <input className="input" style={{ paddingLeft: 30, width: 220 }} placeholder={t("common.search")} aria-label={t("common.search")} value={query} onChange={(e) => setQuery(e.target.value)} />
            </label>
          }
        >
          <div className="table-wrap" style={{ maxHeight: "calc(100vh - 340px)" }}>
            <table className="data">
              <thead>
                <tr>
                  <th>{t("discovery.col.device")}</th>
                  <th>{t("discovery.col.ip")}</th>
                  <th>{t("discovery.col.mac")}</th>
                  <th>{t("discovery.col.os")}</th>
                  <th>{t("discovery.col.ports")}</th>
                  <th className="num">{t("discovery.col.rtt")}</th>
                </tr>
              </thead>
              <tbody>
                {filtered.map((h) => {
                  const Icon = DEVICE_ICON[h.kind];
                  return (
                    <tr key={h.ip} onClick={() => setOpen(h.ip)}>
                      <td>
                        <span className="row" style={{ gap: 10, flexWrap: "nowrap" }}>
                          <span className="device-icon" title={t(`kind.${h.kind}` as Key)}>
                            <Icon size={17} />
                          </span>
                          <span>
                            <span className="cell-strong">{h.name ?? <span className="muted">{t("discovery.unnamed")}</span>}</span>
                            {h.isSelf && <Badge tone="info">{t("discovery.you")}</Badge>}
                            {h.isGateway && <Badge tone="info">{t("discovery.gatewayBadge")}</Badge>}
                            <span className="small muted" style={{ display: "block" }}>
                              {h.model ?? t(`kind.${h.kind}` as Key)}
                            </span>
                          </span>
                        </span>
                      </td>
                      <td className="mono-ish">{h.ip}</td>
                      <td>
                        <span className="mono-ish">{h.mac ?? "—"}</span>
                        <span className="small muted" style={{ display: "block" }}>
                          {h.randomMac ? t("discovery.privateMac") : h.vendor ?? ""}
                        </span>
                      </td>
                      <td>{h.os ?? <span className="muted">—</span>}</td>
                      <td style={{ whiteSpace: "normal", maxWidth: 260 }}>
                        <span className="chips">
                          {h.ports.slice(0, 6).map((p) => (
                            <Badge key={p.port} tone={[23, 21].includes(p.port) ? "warn" : undefined} title={p.service}>
                              {p.port}
                            </Badge>
                          ))}
                          {h.ports.length > 6 && <span className="small muted">+{h.ports.length - 6}</span>}
                        </span>
                      </td>
                      <td className="num">{h.rttMs != null ? `${fmtNumber(lang, h.rttMs, 1)} ms` : "—"}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </Panel>
      )}
      {openHost && <HostDrawer host={openHost} onClose={() => setOpen(null)} />}
    </div>
  );
}
