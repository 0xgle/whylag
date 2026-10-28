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

  const poll = useCallback(async () => {
    try {
      const r = await call<T>(`${prefix}.status`);
      cache.set(prefix, r);
      setRun(r);
      if (r.running) timer.current = setTimeout(poll, interval);
    } catch (e) {
      setError(String((e as Error).message ?? e));
    }
  }, [prefix, interval]);

  useEffect(() => {
    poll();
    return () => clearTimeout(timer.current);
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
