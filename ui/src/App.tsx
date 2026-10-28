import { useEffect, type ComponentType } from "react";
import {
  Activity,
  FileText,
  Gauge,
  LayoutDashboard,
  Map,
  Network,
  Radar,
  Route,
  Settings as SettingsIcon,
  ShieldCheck,
  Wifi,
} from "lucide-react";
import { useApp, type ModuleId } from "./store";
import { useT, type Key } from "./i18n";
import { call } from "./api";
import type { ScanResult } from "./types";
import { Logo } from "./components/Logo";
import { LinkReadout } from "./components/LinkReadout";
import { Notice } from "./components/ui";
import { WifiModule } from "./modules/wifi/WifiModule";
import { SurveyModule } from "./modules/survey/SurveyModule";
import { DiscoveryModule } from "./modules/discovery/DiscoveryModule";
import { PerformanceModule } from "./modules/performance/PerformanceModule";
import { PathModule } from "./modules/path/PathModule";
import { SecurityModule } from "./modules/security/SecurityModule";
import { MonitorModule } from "./modules/monitor/MonitorModule";
import { OverviewModule } from "./modules/overview/OverviewModule";
import { ReportsModule } from "./modules/reports/ReportsModule";
import { SettingsModule } from "./modules/settings/SettingsModule";
import { Placeholder } from "./modules/Placeholder";

type NavEntry = { id: ModuleId; icon: ComponentType<{ size?: number }> } | "gap";

const NAV: NavEntry[] = [
  { id: "overview", icon: LayoutDashboard },
  "gap",
  { id: "wifi", icon: Wifi },
  { id: "survey", icon: Map },
  { id: "discovery", icon: Network },
  { id: "performance", icon: Gauge },
  { id: "path", icon: Route },
  { id: "security", icon: ShieldCheck },
  { id: "monitor", icon: Activity },
  "gap",
  { id: "reports", icon: FileText },
  { id: "settings", icon: SettingsIcon },
];

const MODULES: Partial<Record<ModuleId, ComponentType>> = {
  wifi: WifiModule,
  survey: SurveyModule,
  discovery: DiscoveryModule,
  performance: PerformanceModule,
  path: PathModule,
  security: SecurityModule,
  monitor: MonitorModule,
  overview: OverviewModule,
  reports: ReportsModule,
  settings: SettingsModule,
};

/** Polls the Wi-Fi scanner for the header readout and the analyzer. */
function useScanPoller() {
  const paused = useApp((s) => s.scanPaused);
  const dataSource = useApp((s) => s.settings?.dataSource);
  useEffect(() => {
    if (paused) return;
    let alive = true;
    let timer: ReturnType<typeof setTimeout>;
    const tick = async () => {
      try {
        const r = await call<ScanResult>("wifi.scan");
        if (alive) useApp.getState().pushScan(r);
      } catch (e) {
        const err = e as { code?: string; message?: string };
        if (alive) useApp.getState().setScanError({ code: err.code ?? "bridge", message: err.message ?? String(e) });
      }
      if (alive) timer = setTimeout(tick, 3000);
    };
    tick();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [paused, dataSource]);
}

export function App() {
  const t = useT();
  const module = useApp((s) => s.module);
  const setModule = useApp((s) => s.setModule);
  const theme = useApp((s) => s.theme);
  const lang = useApp((s) => s.lang);
  const bridgeError = useApp((s) => s.bridgeError);

  useEffect(() => {
    useApp.getState().init();
  }, []);
  useEffect(() => {
    if (theme === "system") document.documentElement.removeAttribute("data-theme");
    else document.documentElement.setAttribute("data-theme", theme);
  }, [theme]);
  useEffect(() => {
    document.documentElement.lang = lang;
  }, [lang]);
  useScanPoller();

  const Current = MODULES[module] ?? Placeholder;
  const info = useApp((s) => s.appInfo);

  return (
    <div className="shell">
      <nav className="rail" aria-label="Modules">
        <div className="wordmark">
          <Logo />
          <div>
            <div className="wordmark-name">WhyLag</div>
            <div className="wordmark-tag">{t("app.tagline")}</div>
          </div>
        </div>
        {NAV.map((entry, i) =>
          entry === "gap" ? (
            <div key={`gap-${i}`} className="nav-gap" />
          ) : (
            <button
              key={entry.id}
              className="nav-item"
              aria-current={module === entry.id ? "page" : undefined}
              onClick={() => setModule(entry.id)}
            >
              <entry.icon size={18} />
              {t(`nav.${entry.id}` as Key)}
            </button>
          ),
        )}
        <div className="rail-foot">
          <Radar size={12} style={{ verticalAlign: -1, marginRight: 5 }} aria-hidden />
          WhyLag {info?.version ?? ""}
        </div>
      </nav>
      <header className="header">
        <h1 className="header-title">{t(`nav.${module}` as Key)}</h1>
        <LinkReadout />
      </header>
      <main className="main">
        {bridgeError && (
          <div style={{ marginBottom: 16 }}>
            <Notice tone="crit">{t("notice.bridge")}</Notice>
          </div>
        )}
        <Current />
      </main>
    </div>
  );
}
