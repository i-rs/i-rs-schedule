import { useEffect, useRef, useState } from "react";
import { getLiveOutput, type LiveSnapshot } from "@/api";
import { t } from "@/lib/i18n";

function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(2)} MB`;
}

/**
 * 终端风格实时输出视图:长轮询 /live 直到 done;
 * 快照整体渲染 + 自动滚到底部,断线退避重连。
 */
export function LiveTerminal({ execId, onDone }: { execId: string; onDone?: () => void }) {
  const [snap, setSnap] = useState<LiveSnapshot | null>(null);
  const preRef = useRef<HTMLPreElement>(null);

  useEffect(() => {
    const controller = new AbortController();
    let alive = true;
    (async () => {
      let cursor = 0;
      let failures = 0;
      while (alive) {
        try {
          const s = await getLiveOutput(execId, cursor, controller.signal);
          if (!alive) return;
          setSnap(s);
          cursor = s.version;
          failures = 0;
          if (s.done) {
            onDone?.();
            return;
          }
        } catch {
          if (!alive) return;
          failures += 1;
          await new Promise((r) => setTimeout(r, Math.min(2000 * failures, 10000)));
        }
      }
    })();
    return () => {
      alive = false;
      controller.abort();
    };
    // oxlint-disable-next-line exhaustive-deps -- onDone 由调用方保证稳定
  }, [execId]);

  useEffect(() => {
    if (preRef.current) preRef.current.scrollTop = preRef.current.scrollHeight;
  }, [snap?.output]);

  const done = snap?.done ?? false;
  const waiting = !done && (snap?.output ?? "") === "";
  return (
    <div className="rounded-md border border-border/60 bg-zinc-950 overflow-hidden">
      <div className="flex items-center justify-between px-2.5 py-1.5 border-b border-border/40 bg-zinc-900/60">
        <span className={`inline-flex items-center gap-1.5 text-[10px] font-medium uppercase tracking-wider ${done ? "text-zinc-400" : "text-emerald-400"}`}>
          {!done && (
            <span className="relative flex h-1.5 w-1.5">
              <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75" />
              <span className="relative inline-flex rounded-full h-1.5 w-1.5 bg-emerald-500" />
            </span>
          )}
          {done ? t("Finished") : t("Live")}
        </span>
        {snap?.total != null && (
          <span className="text-[10px] text-zinc-500 tabular-nums">{fmtBytes(snap.total)}</span>
        )}
      </div>
      <pre
        ref={preRef}
        className="p-2.5 text-xs font-mono text-zinc-300 max-h-48 overflow-auto whitespace-pre-wrap leading-relaxed"
      >
        {snap?.output}
        {waiting && <span className="text-zinc-500">{t("Waiting for response…")}</span>}
        {!done && (
          <span className="inline-block w-1.5 h-3.5 bg-emerald-400/80 animate-pulse align-middle ml-0.5" />
        )}
      </pre>
    </div>
  );
}
