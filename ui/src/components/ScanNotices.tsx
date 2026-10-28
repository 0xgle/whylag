import { useApp } from "../store";
import { useLang, translateMaybe, useT } from "../i18n";
import { openExternal } from "../api";
import { Notice } from "./ui";
import type { Notice as N } from "../types";

const TONE: Record<string, "info" | "warn" | "crit"> = {
  simulator: "info",
  fellBackToSimulator: "warn",
  locationPermission: "warn",
  noWifiAdapter: "warn",
  wlanApi: "crit",
};

/** Explains where scan data comes from and how to fix missing permissions. */
export function ScanNotices() {
  const t = useT();
  const lang = useLang();
  const scan = useApp((s) => s.scan);
  const scanError = useApp((s) => s.scanError);
  const notices: N[] = [...(scan?.notices ?? [])];
  if (scanError) notices.push({ code: scanError.code, detail: scanError.message });
  // When we fell back to the simulator, fold the reason into that one notice
  // instead of stacking several banners.
  const codes = new Set(notices.map((n) => n.code));
  const fallback = codes.has("fellBackToSimulator") || codes.has("noWifiAdapter");
  const reasons = notices.filter((n) => !["simulator", "fellBackToSimulator", "locationPermission", "noWifiAdapter"].includes(n.code));
  let shown = notices.filter((n, i) => notices.findIndex((m) => m.code === n.code) === i);
  if (fallback) {
    shown = shown.filter((n) => n.code === "fellBackToSimulator" || n.code === "locationPermission" || n.code === "noWifiAdapter");
    if (codes.has("noWifiAdapter")) shown = shown.filter((n) => n.code !== "fellBackToSimulator");
  }
  if (!shown.length) return null;
  return (
    <div className="stack" style={{ gap: 8 }}>
      {shown.map((n) => (
        <Notice
          key={n.code}
          tone={TONE[n.code] ?? "info"}
          action={
            n.code === "locationPermission" ? (
              <button className="btn btn-sm" onClick={() => openExternal("ms-settings:privacy-location")}>
                {t("notice.locationPermission.action")}
              </button>
            ) : undefined
          }
        >
          {translateMaybe(lang, `notice.${n.code}`, n.detail || n.code)}
          {n.code === "fellBackToSimulator" && reasons.length > 0 && (
            <span className="small" style={{ opacity: 0.8 }}>
              {" "}
              {reasons.map((r) => translateMaybe(lang, `notice.${r.code}`, r.code)).join(" ")}
            </span>
          )}
          {n.detail && !fallback && !["simulator"].includes(n.code) && (
            <span className="small" style={{ opacity: 0.75 }}> ({n.detail})</span>
          )}
        </Notice>
      ))}
    </div>
  );
}
