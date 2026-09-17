import { useEffect, useRef } from "react";
import { pollEvents } from "@/api";

/**
 * 订阅全局事件长轮询:任何执行/任务变化时触发 onEvent(200ms 防抖)。
 * 首次响应只同步游标不触发回调,避免与页面初始加载重复;
 * 断线按 2s 起步退避重连,自愈。
 */
export function useEvents(onEvent: () => void) {
  const cbRef = useRef(onEvent);
  cbRef.current = onEvent;

  useEffect(() => {
    const controller = new AbortController();
    let alive = true;
    let debounce: ReturnType<typeof setTimeout> | undefined;
    const fire = () => {
      clearTimeout(debounce);
      debounce = setTimeout(() => cbRef.current(), 200);
    };

    (async () => {
      let cursor: number | null = null;
      let failures = 0;
      while (alive) {
        try {
          const { cursor: next } = await pollEvents(cursor ?? 0, controller.signal);
          if (!alive) return;
          if (cursor !== null && next > cursor) fire();
          cursor = next;
          failures = 0;
        } catch {
          if (!alive) return;
          failures += 1;
          await new Promise((r) => setTimeout(r, Math.min(2000 * failures, 10000)));
        }
      }
    })();

    return () => {
      alive = false;
      clearTimeout(debounce);
      controller.abort();
    };
  }, []);
}
