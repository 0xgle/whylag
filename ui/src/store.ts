import { create } from "zustand";
import { call } from "./api";
import type { AppInfo, ScanResult, Settings } from "./types";

export type ModuleId =
  | "overview"
  | "wifi"
  | "survey"
  | "discovery"
  | "performance"
  | "path"
  | "security"
  | "monitor"
  | "reports"
  | "settings";

export type Theme = "system" | "light" | "dark";

export interface RssiPoint {
  t: number;
  rssi: number;
}

/** How long signal history is kept in memory for the time charts. */
const HISTORY_MS = 5 * 60 * 1000;

interface AppState {
  lang: "en" | "pl";
  theme: Theme;
  module: ModuleId;
  settings: Settings | null;
  appInfo: AppInfo | null;
  bridgeError: string | null;

  scan: ScanResult | null;
  scanError: { code: string; message: string } | null;
  scanPaused: boolean;
  history: Record<string, RssiPoint[]>;

  setModule: (m: ModuleId) => void;
  setTheme: (t: Theme) => void;
  init: () => Promise<void>;
  updateSettings: (patch: Partial<Settings>) => Promise<void>;
  pushScan: (r: ScanResult) => void;
  setScanError: (e: { code: string; message: string } | null) => void;
  setScanPaused: (p: boolean) => void;
  clearHistory: () => void;
}

function storedTheme(): Theme {
  try {
    const t = localStorage.getItem("whylag.theme");
    if (t === "light" || t === "dark" || t === "system") return t;
  } catch {
    /* storage unavailable */
  }
  return "system";
}

function storedModule(): ModuleId {
  try {
    const m = localStorage.getItem("whylag.module");
    if (m) return m as ModuleId;
  } catch {
    /* storage unavailable */
  }
  return "overview";
}

export const useApp = create<AppState>((set, get) => ({
  lang: navigator.language?.toLowerCase().startsWith("pl") ? "pl" : "en",
  theme: storedTheme(),
  module: storedModule(),
  settings: null,
  appInfo: null,
  bridgeError: null,

  scan: null,
  scanError: null,
  scanPaused: false,
  history: {},

  setModule: (module) => {
    try {
      localStorage.setItem("whylag.module", module);
    } catch {
      /* storage unavailable */
    }
    set({ module });
  },
  setTheme: (theme) => {
    try {
      localStorage.setItem("whylag.theme", theme);
    } catch {
      /* storage unavailable */
    }
    set({ theme });
  },
  init: async () => {
    try {
      const [settings, appInfo] = await Promise.all([call<Settings>("settings.get"), call<AppInfo>("app.info")]);
      set({ settings, appInfo, lang: settings.language === "pl" ? "pl" : "en", bridgeError: null });
    } catch (e) {
      set({ bridgeError: String(e) });
    }
  },
  updateSettings: async (patch) => {
    const settings = await call<Settings>("settings.set", patch);
    set({ settings, lang: settings.language === "pl" ? "pl" : "en" });
  },
  pushScan: (r) => {
    const cutoff = r.timestampMs - HISTORY_MS;
    const history = { ...get().history };
    for (const n of r.networks) {
      const prev = history[n.bssid] ?? [];
      const next = prev.length && prev[0].t < cutoff ? prev.filter((p) => p.t >= cutoff) : prev.slice();
      next.push({ t: r.timestampMs, rssi: n.rssi });
      history[n.bssid] = next;
    }
    // Drop networks not seen within the window.
    for (const k of Object.keys(history)) {
      const pts = history[k];
      if (!pts.length || pts[pts.length - 1].t < cutoff) delete history[k];
    }
    set({ scan: r, history, scanError: null });
  },
  setScanError: (scanError) => set({ scanError }),
  setScanPaused: (scanPaused) => set({ scanPaused }),
  clearHistory: () => set({ history: {} }),
}));
