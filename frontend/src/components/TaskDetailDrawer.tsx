import { useEffect, useState } from "react";
import { Sheet, SheetContent, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Skeleton } from "@/components/ui/skeleton";
import { getTaskStats, listTaskExecutions, enableHook, disableHook, getHook, type Task, type TaskExecution } from "@/api";
import { useApi } from "@/hooks/useApi";
import { LiveTerminal } from "@/components/LiveTerminal";
import { toast } from "@/hooks/useToast";
import { t, tf, useLang } from "@/lib/i18n";
import { timeAgo, fullTime } from "@/lib/time";
import { fmtDuration } from "@/lib/duration";
import { Globe, Terminal, Clock, Bell, Link2, ChevronDown, ChevronUp, Webhook, Copy, Check } from "lucide-react";

function fmtMs(ms: number | null): string {
  if (ms == null) return "—";
  if (ms < 1000) return `${Math.round(ms)}ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
  return `${Math.floor(ms / 60000)}m ${Math.round((ms % 60000) / 1000)}s`;
}

function ExecutionRow({ e, onChanged }: { e: TaskExecution; onChanged: () => void }) {
  const [expanded, setExpanded] = useState(false);
  const ok = e.status === "success";
  const running = e.status === "running";
  return (
    <div className="rounded-md border border-border/50 px-2.5 py-2 text-sm space-y-1.5">
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2 min-w-0">
          <span
            className={`h-2 w-2 shrink-0 rounded-full ${ok ? "bg-emerald-500" : running ? "bg-blue-500" : e.status === "skipped" ? "bg-amber-500" : "bg-destructive"} ${running ? "animate-pulse" : ""}`}
          />
          <span className={`text-xs font-medium ${ok ? "text-emerald-500" : running ? "text-blue-500" : e.status === "skipped" ? "text-amber-500" : "text-destructive"}`}>
            {t(e.status)}
          </span>
          {e.attempt > 0 && (
            <span className="text-xs text-amber-500 tabular-nums">{tf("attempt {n}", { n: e.attempt + 1 })}</span>
          )}
          {e.http_status && <span className="text-xs text-muted-foreground tabular-nums">HTTP {e.http_status}</span>}
        </div>
        <div className="flex items-center gap-2 text-xs text-muted-foreground tabular-nums whitespace-nowrap">
          {e.finished_at && e.started_at && <span>{fmtDuration(e.started_at, e.finished_at)}</span>}
          <span title={fullTime(e.started_at)}>{timeAgo(e.started_at)}</span>
        </div>
      </div>
      {running && <LiveTerminal execId={e.id} onDone={onChanged} />}
      {e.output && !running && (
        <div>
          <button
            onClick={() => setExpanded(!expanded)}
            className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground cursor-pointer"
          >
            {expanded ? <ChevronUp className="h-3 w-3" /> : <ChevronDown className="h-3 w-3" />}
            {expanded ? t("Hide output") : t("Show output")}
          </button>
          {expanded && (
            <pre className="mt-1.5 rounded-md border border-border/50 bg-muted/50 p-2 text-xs text-muted-foreground font-mono max-h-40 overflow-auto whitespace-pre-wrap">
              {e.output}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}

export function TaskDetailDrawer({ task, onClose }: { task: Task | null; onClose: () => void }) {
  const [statusFilter, setStatusFilter] = useState("all");
  const [limit, setLimit] = useState(20);
  const lang = useLang();
  void lang;

  const [hookState, setHookState] = useState<{ enabled: boolean } | null>(null);
  const [hookUrl, setHookUrl] = useState<string | null>(null);
  const [hookCopied, setHookCopied] = useState(false);
  const [hookBusy, setHookBusy] = useState(false);

  useEffect(() => {
    if (!task) {
      setHookState(null);
      setHookUrl(null);
      return;
    }
    getHook(task.id)
      .then(setHookState)
      .catch(() => setHookState(null));
    setHookUrl(null);
  }, [task?.id]);

  const { data: stats, reload: statsReload } = useApi(
    () => (task ? getTaskStats(task.id) : Promise.resolve(null)),
    [task?.id],
  );
  const { data: execData, loading, reload } = useApi<TaskExecution[]>(
    () => (task ? listTaskExecutions(task.id, limit) : Promise.resolve([])),
    [task?.id, limit],
  );
  const execs = execData ?? [];
  const visible = statusFilter === "all" ? execs : execs.filter((e) => e.status === statusFilter);
  const atEnd = execs.length < limit;

  const TypeIcon = task?.task_type.type === "http" ? Globe : Terminal;

  return (
    <Sheet open={task !== null} onOpenChange={(open) => !open && onClose()}>
      <SheetContent className="sm:max-w-md w-full overflow-y-auto gap-0" showCloseButton>
        {task && (
          <>
            <SheetHeader className="space-y-2 pb-0">
              <div className="flex items-center gap-2">
                <span className={`flex h-7 w-7 items-center justify-center rounded-md ${task.task_type.type === "http" ? "bg-primary/10 text-primary" : "bg-violet-500/10 text-violet-500"}`}>
                  <TypeIcon className="h-4 w-4" />
                </span>
                <SheetTitle className="text-lg">{task.name}</SheetTitle>
                <Badge variant={task.enabled ? "default" : "secondary"} className="text-[10px]">
                  {task.enabled ? t("Enabled") : t("Disabled")}
                </Badge>
              </div>
              <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
                <span className="inline-flex items-center gap-1">
                  <Clock className="h-3 w-3" />
                  {task.schedule.type === "cron" ? task.schedule.expr : `${task.schedule.delay_secs}s`}
                </span>
                <span>{task.timezone}</span>
                <span>{t("Timeout (s)")} {task.timeout_secs}</span>
                {task.max_retries > 0 && <span>{t("Retries")} {task.max_retries}</span>}
                {task.notify_type !== "none" && (
                  <span className="inline-flex items-center gap-1 text-amber-500">
                    <Bell className="h-3 w-3" /> {task.notify_type}
                  </span>
                )}
                {task.trigger_task_ids.length > 0 && (
                  <span className="inline-flex items-center gap-1 text-sky-400">
                    <Link2 className="h-3 w-3" /> {t("Trigger chain")}
                  </span>
                )}
                {(task.tags ?? []).map((tag) => (
                  <span key={tag} className="rounded-full px-1.5 py-px text-[10px] font-medium bg-primary/10 text-primary">
                    {tag}
                  </span>
                ))}
              </div>
            </SheetHeader>

            <div className="px-4 space-y-5 pb-6">
              {/* 统计 */}
              <div className="grid grid-cols-3 gap-2">
                {[
                  { label: t("Total runs"), value: String(stats?.total ?? 0) },
                  { label: t("Success"), value: String(stats?.success ?? 0), cls: "text-emerald-500" },
                  { label: t("Failed"), value: String(stats?.failure ?? 0), cls: "text-destructive" },
                ].map((s) => (
                  <div key={s.label} className="rounded-lg border border-border/50 p-2.5 text-center">
                    <div className={`text-xl font-semibold tabular-nums ${s.cls ?? ""}`}>{s.value}</div>
                    <div className="text-[10px] text-muted-foreground mt-0.5">{s.label}</div>
                  </div>
                ))}
              </div>
              <div className="text-xs text-muted-foreground -mt-3">
                {t("Avg duration")}: <span className="tabular-nums">{fmtMs(stats?.avg_duration_ms ?? null)}</span>
              </div>

              {/* Webhook 触发 */}
              <div className="space-y-2">
                <div className="flex items-center justify-between">
                  <p className="text-sm font-medium inline-flex items-center gap-1.5">
                    <Webhook className="h-3.5 w-3.5" /> {t("Webhook trigger")}
                  </p>
                  {hookState?.enabled && !hookUrl && (
                    <span className="text-[10px] text-emerald-500 font-medium">● {t("Enabled")}</span>
                  )}
                </div>
                {hookUrl ? (
                  <div className="rounded-md border border-primary/40 bg-primary/5 p-2 space-y-1.5">
                    <p className="text-[10px] text-muted-foreground">
                      {t("Copy this URL now — the secret is shown only once.")}
                    </p>
                    <div className="flex items-center gap-1.5 min-w-0">
                      <code className="text-[10px] font-mono truncate flex-1 break-all">{hookUrl}</code>
                      <button
                        title={t("Copy")}
                        onClick={async () => {
                          await navigator.clipboard.writeText(hookUrl);
                          setHookCopied(true);
                          setTimeout(() => setHookCopied(false), 1500);
                        }}
                        className="shrink-0 text-muted-foreground hover:text-foreground cursor-pointer"
                      >
                        {hookCopied ? <Check className="h-3.5 w-3.5 text-emerald-500" /> : <Copy className="h-3.5 w-3.5" />}
                      </button>
                    </div>
                  </div>
                ) : (
                  <div className="flex items-center gap-2">
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={hookBusy}
                      className="h-7 text-xs"
                      onClick={async () => {
                        if (!task) return;
                        setHookBusy(true);
                        try {
                          const r = await enableHook(task.id);
                          setHookUrl(`${window.location.origin}${r.path}`);
                          setHookState({ enabled: true });
                        } catch (e) {
                          toast.error(e instanceof Error ? e.message : String(e));
                        } finally {
                          setHookBusy(false);
                        }
                      }}
                    >
                      {hookState?.enabled ? t("Rotate secret") : t("Enable")}
                    </Button>
                    {hookState?.enabled && (
                      <Button
                        size="sm"
                        variant="ghost"
                        disabled={hookBusy}
                        className="h-7 text-xs text-destructive"
                        onClick={async () => {
                          if (!task) return;
                          setHookBusy(true);
                          try {
                            await disableHook(task.id);
                            setHookState({ enabled: false });
                          } catch (e) {
                            toast.error(e instanceof Error ? e.message : String(e));
                          } finally {
                            setHookBusy(false);
                          }
                        }}
                      >
                        {t("Disable")}
                      </Button>
                    )}
                  </div>
                )}
                <p className="text-[10px] text-muted-foreground">
                  {t("POST to this URL to run the task. Use {{event.body}} / {{event.query.x}} in the command.")}
                </p>
              </div>

              {/* 执行历史 */}
              <div className="space-y-2">
                <div className="flex items-center justify-between">
                  <p className="text-sm font-medium">{t("Recent Executions")}</p>
                  <Tabs value={statusFilter} onValueChange={setStatusFilter}>
                    <TabsList className="h-7">
                      <TabsTrigger value="all" className="text-xs px-2">{t("All")}</TabsTrigger>
                      <TabsTrigger value="success" className="text-xs px-2">{t("Success")}</TabsTrigger>
                      <TabsTrigger value="failure" className="text-xs px-2">{t("Failure")}</TabsTrigger>
                      <TabsTrigger value="skipped" className="text-xs px-2">{t("Skipped")}</TabsTrigger>
                    </TabsList>
                  </Tabs>
                </div>
                {loading ? (
                  <div className="space-y-2">
                    {[1, 2, 3].map((i) => <Skeleton key={i} className="h-14" />)}
                  </div>
                ) : visible.length === 0 ? (
                  <p className="text-xs text-muted-foreground py-6 text-center">{t("No executions yet.")}</p>
                ) : (
                  <div className="space-y-2">
                    {visible.map((e) => (
                      <ExecutionRow
                        key={e.id}
                        e={e}
                        onChanged={() => {
                          reload();
                          statsReload();
                        }}
                      />
                    ))}
                    {!atEnd && (
                      <div className="flex justify-center pt-1">
                        <Button variant="outline" size="sm" onClick={() => setLimit((l) => l + 20)} disabled={loading}>
                          <ChevronDown className="h-4 w-4" /> {t("Load More")}
                        </Button>
                      </div>
                    )}
                  </div>
                )}
              </div>
            </div>
          </>
        )}
      </SheetContent>
    </Sheet>
  );
}
