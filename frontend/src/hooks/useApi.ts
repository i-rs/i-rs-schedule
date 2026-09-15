import { useCallback, useEffect, useRef, useState } from "react";

/**
 * 统一的数据获取模式:loading/error/data/reload。
 * deps 变化时自动重新获取(如 Executions 的 filter/limit)。
 */
export function useApi<T>(fn: () => Promise<T>, deps: unknown[]) {
  const [data, setData] = useState<T | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);
  const alive = useRef(true);

  // eslint-disable-next-line react-hooks/exhaustive-deps
  const run = useCallback(async () => {
    setLoading(true);
    try {
      const result = await fn();
      if (alive.current) {
        setData(result);
        setError(null);
      }
    } catch (e) {
      if (alive.current) {
        setError(e instanceof Error ? e : new Error(String(e)));
      }
    } finally {
      if (alive.current) {
        setLoading(false);
      }
    }
  }, deps);

  useEffect(() => {
    alive.current = true;
    run();
    return () => {
      alive.current = false;
    };
  }, [run]);

  return { data, loading, error, reload: run, setData };
}
