import { useCallback, useEffect, useRef, useState } from "react";
import { call } from "../api";

/**
 * Background jobs (discovery, performance, path) run in the engine; the UI
 * starts them and polls their status until they finish. The last result stays
 * cached per module so switching tabs does not lose it.
 */
const cache = new Map<string, unknown>();

export function useJob<T extends { running: boolean }>(prefix: string, interval = 400) {
  const [run, setRun] = useState<T | null>((cache.get(prefix) as T) ?? null);
  const [error, setError] = useState<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  // Polls resolving after unmount must not schedule more polls.
  const alive = useRef(true);
  const failures = useRef(0);

  const poll = useCallback(async () => {
    try {
      const r = await call<T>(`${prefix}.status`);
      if (!alive.current) return;
      failures.current = 0;
      setError(null);
      cache.set(prefix, r);
      setRun(r);
      if (r.running) timer.current = setTimeout(poll, interval);
    } catch (e) {
      if (!alive.current) return;
      setError(String((e as Error).message ?? e));
      // A missed poll (busy engine, bridge restarting) should not freeze the
      // progress view: retry with a growing delay.
      failures.current += 1;
      if (failures.current <= 10) timer.current = setTimeout(poll, interval * Math.min(8, 2 ** failures.current));
    }
  }, [prefix, interval]);

  useEffect(() => {
    alive.current = true;
    poll();
    return () => {
      alive.current = false;
      clearTimeout(timer.current);
    };
  }, [poll]);

  const start = useCallback(
    async (params: unknown = {}) => {
      setError(null);
      try {
        const r = await call<T>(`${prefix}.start`, params);
        cache.set(prefix, r);
        setRun(r);
        clearTimeout(timer.current);
        timer.current = setTimeout(poll, interval);
      } catch (e) {
        setError(String((e as Error).message ?? e));
      }
    },
    [prefix, poll, interval],
  );

  const stop = useCallback(async () => {
    await call(`${prefix}.stop`).catch(() => {});
  }, [prefix]);

  return { run, start, stop, error };
}
