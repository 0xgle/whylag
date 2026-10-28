import { useEffect, useState } from "react";
import { Download } from "lucide-react";
import { isTauri } from "../api";
import { useT } from "../i18n";

type Pending = { version: string; install: () => Promise<void> };

/**
 * Checks GitHub releases for a newer signed build once per start and offers
 * to install it. Desktop app only; silent when offline or up to date.
 */
export function UpdateBanner() {
  const t = useT();
  const [pending, setPending] = useState<Pending | null>(null);
  const [state, setState] = useState<"idle" | "installing" | "failed">("idle");

  useEffect(() => {
    if (!isTauri) return;
    let alive = true;
    (async () => {
      try {
        const { check } = await import("@tauri-apps/plugin-updater");
        const update = await check();
        if (!update || !alive) return;
        setPending({
          version: update.version,
          install: async () => {
            await update.downloadAndInstall();
            const { relaunch } = await import("@tauri-apps/plugin-process");
            await relaunch();
          },
        });
      } catch {
        /* offline, or no signed update feed yet */
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  if (!pending) return null;
  const install = async () => {
    setState("installing");
    try {
      await pending.install();
    } catch {
      setState("failed");
    }
  };
  return (
    <div className="update-banner" role="status">
      <span>{state === "failed" ? t("update.failed") : t("update.available", { version: pending.version })}</span>
      <button className="btn btn-primary" onClick={install} disabled={state === "installing"}>
        <Download size={15} />
        {state === "installing" ? t("update.installing") : t("update.install")}
      </button>
      <button className="btn btn-ghost" onClick={() => setPending(null)}>
        {t("update.later")}
      </button>
    </div>
  );
}
