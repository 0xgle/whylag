import { useEffect, useMemo, useRef, useState } from "react";
import { Settings2, X } from "lucide-react";
import { call, isTauri } from "../../api";
import { useApp } from "../../store";
import { fmtNumber, useLang, useT, type Key } from "../../i18n";
import { median } from "./stats";

interface LiveSample {
  t: number;
  gatewayMs: number | null;
  targetMs: number | null;
  target: string;
  rssi: number | null;
  ssid: string | null;
  wifi: boolean;
  simulated: boolean;
}

const WINDOW = 20;
const TARGET_KEY = "whylag.overlay.target";

function readTarget() {
  try {
    return localStorage.getItem(TARGET_KEY) || "1.1.1.1";
  } catch {
    return "1.1.1.1";
  }
}

/** Mean absolute difference between consecutive answers, the usual jitter figure. */
function jitter(v: number[]) {
  if (v.length < 2) return null;
  let s = 0;
  for (let i = 1; i < v.length; i++) s += Math.abs(v[i] - v[i - 1]);
  return s / (v.length - 1);
}

function stats(values: (number | null)[]) {
  const ok = values.filter((v): v is number => v != null);
  return { med: median(ok), jit: jitter(ok), loss: values.length ? (100 * (values.length - ok.length)) / values.length : 0 };
}

export type Cause = "ok" | "wifi" | "home" | "isp" | "offline";

/**
 * Where lag comes from, judged from the last seconds: if the router itself is
 * slow or drops answers the problem is local (weak Wi-Fi or the home network),
 * otherwise it is past the router, at the provider or beyond.
 */
export function diagnose(samples: LiveSample[]): { level: "good" | "fair" | "bad"; cause: Cause } {
  const tg = stats(samples.map((s) => s.targetMs));
  const gw = stats(samples.map((s) => s.gatewayMs));
  const last = samples[samples.length - 1];
  if (samples.length >= 3 && samples.slice(-3).every((s) => s.targetMs == null && s.gatewayMs == null)) return { level: "bad", cause: "offline" };
  const bad = tg.loss > 5 || (tg.med ?? 0) > 120 || (tg.jit ?? 0) > 30;
  const fair = !bad && (tg.loss > 0 || (tg.med ?? 0) > 60 || (tg.jit ?? 0) > 12);
  if (!bad && !fair) return { level: "good", cause: "ok" };
  // No router answers at all while the target does: the router is unknown or
  // ignores probes, which says nothing about where the lag is.
  const gwKnown = samples.some((s) => s.gatewayMs != null);
  const localTrouble = gwKnown && (gw.loss > 3 || (gw.jit ?? 0) > 10 || (gw.med ?? 0) > 25);
  const weak = last?.rssi != null && last.rssi < -70;
  const cause: Cause = localTrouble || weak ? (last?.wifi ? "wifi" : "home") : "isp";
  return { level: bad ? "bad" : "fair", cause };
}

export function Overlay() {
  const t = useT();
  const lang = useLang();
  const [samples, setSamples] = useState<LiveSample[]>([]);
  const [target, setTarget] = useState(readTarget);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(target);
  const busy = useRef(false);

  useEffect(() => {
    useApp.getState().init();
    document.documentElement.classList.add("overlay-root");
    // Native windows are not transparent: square corners avoid white edges.
    if (isTauri) document.documentElement.classList.add("overlay-native");
  }, []);

  useEffect(() => {
    let alive = true;
    const tick = async () => {
      if (busy.current) return;
      busy.current = true;
      try {
        const s = await call<LiveSample>("live.sample", { target });
        if (alive) setSamples((cur) => [...cur.slice(-(WINDOW - 1)), s]);
      } catch {
        /* the next tick retries */
      } finally {
        busy.current = false;
      }
    };
    tick();
    const id = setInterval(tick, 1000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, [target]);

  const tg = useMemo(() => stats(samples.map((s) => s.targetMs)), [samples]);
  const verdict = useMemo(() => diagnose(samples), [samples]);
  const last = samples[samples.length - 1];
  const now = last?.targetMs;
  const maxBar = Math.max(60, ...samples.map((s) => s.targetMs ?? 0));

  const close = async () => {
    if (isTauri) {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().close();
    } else window.close();
  };
  const save = () => {
    const v = draft.trim() || "1.1.1.1";
    try {
      localStorage.setItem(TARGET_KEY, v);
    } catch {
      /* storage unavailable */
    }
    setSamples([]);
    setTarget(v);
    setEditing(false);
  };

  return (
    <div className={`overlay overlay-${verdict.level}`} data-tauri-drag-region>
      <div className="overlay-top" data-tauri-drag-region>
        <span className="overlay-target" data-tauri-drag-region>
          {last?.simulated ? `${t("overlay.demo")} · ` : ""}
          {target}
        </span>
        <button className="overlay-icon" onClick={() => setEditing(!editing)} aria-label={t("overlay.target")} title={t("overlay.target")}>
          <Settings2 size={13} />
        </button>
        <button className="overlay-icon" onClick={close} aria-label={t("common.close")} title={t("common.close")}>
          <X size={13} />
        </button>
      </div>
      {editing ? (
        <form
          className="overlay-edit"
          onSubmit={(e) => {
            e.preventDefault();
            save();
          }}
        >
          <label>
            <span>{t("overlay.target")}</span>
            <input className="input" autoFocus value={draft} onChange={(e) => setDraft(e.target.value)} placeholder="1.1.1.1" />
          </label>
          <button className="btn btn-primary btn-sm" type="submit">
            {t("overlay.save")}
          </button>
        </form>
      ) : (
        <>
          <div className="overlay-main" data-tauri-drag-region>
            <span className="overlay-ms" data-tauri-drag-region>
              <b>{now != null ? fmtNumber(lang, now) : "—"}</b> ms
            </span>
            <span className="overlay-cause" data-tauri-drag-region>
              {t(`overlay.cause.${verdict.cause}` as Key)}
            </span>
          </div>
          <div className="overlay-bars" aria-hidden data-tauri-drag-region>
            {Array.from({ length: WINDOW }, (_, i) => samples[samples.length - WINDOW + i]).map((s, i) => (
              <span
                key={i}
                className={s && s.targetMs == null ? "lost" : ""}
                style={{ height: s ? `${s.targetMs == null ? 100 : Math.max(6, (100 * s.targetMs) / maxBar)}%` : "0%" }}
              />
            ))}
          </div>
          <div className="overlay-foot" data-tauri-drag-region>
            {t("overlay.stats", {
              jitter: tg.jit != null ? fmtNumber(lang, tg.jit) : "—",
              loss: fmtNumber(lang, tg.loss),
            })}
            {last?.rssi != null && ` · ${last.rssi} dBm`}
          </div>
        </>
      )}
    </div>
  );
}
