import { useState, useEffect } from "react";
import { Card, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from "@/components/ui/dialog";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Skeleton } from "@/components/ui/skeleton";
import { listExecutions, listTasks, type TaskExecution, type Task } from "@/api";
import { toast } from "@/hooks/useToast";
import { useApi } from "@/hooks/useApi";
import { t, tf, useLang } from "@/lib/i18n";
import { timeAgo } from "@/lib/time";
import { fmtDuration } from "@/lib/duration";
import { RefreshCw, CheckCircle2, XCircle, Clock, PauseCircle, ChevronDown, ChevronUp, Inbox } from "lucide-react";

const statusConfig: Record<string, { icon: typeof CheckCircle2; color: string; border: string }> = {
  success: { icon: CheckCircle2, color: "text-emerald-500", border: "border-l-emerald-500" },
  failure: { icon: XCircle, color: "text-red-500", border: "border-l-red-500" },
  running: { icon: Clock, color: "text-blue-500", border: "border-l-blue-500" },
  interrupted: { icon: PauseCircle, color: "text-amber-500", border: "border-l-amber-500" },
  skipped: { icon: PauseCircle, color: "text-muted-foreground", border: "border-l-muted-foreground" },
};

function fmtTime(iso: string) {
  const d = new Date(iso);
  return d.toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

function ExecutionCard({ e, task, onOpen }: { e: TaskExecution; task?: Task; onOpen?: () => void }) {
  const [expanded, setExpanded] = useState(false);
  const cfg = statusConfig[e.status] ?? statusConfig.failure;
  const Icon = cfg.icon;
  const duration = fmtDuration(e.started_at, e.finished_at);

  return (
    <Card
      onClick={onOpen}
      className={`cursor-pointer border-l-4 ${cfg.border} shadow-[var(--shadow-card)] transition-all duration-200 hover:-translate-y-0.5 hover:shadow-[var(--shadow-card-hover)]`}
    >
      <CardContent className="py-4">
        <div className="flex items-start justify-between gap-4">
          <div className="flex-1 min-w-0 space-y-2">
            <div className="flex flex-wrap items-center gap-2">
              <span className={`flex h-6 w-6 items-center justify-center rounded-md bg-current/10 ${cfg.color}`}>
                <Icon className="h-3.5 w-3.5" />
              </span>
              <Badge variant={e.status === "success" ? "default" : e.status === "running" ? "outline" : "destructive"}>
                {t(e.status)}
              </Badge>
              {task && <span className="text-sm font-medium truncate">{task.name}</span>}
              {e.attempt > 0 && (
                <span className="text-xs text-amber-500 tabular-nums">
                  {tf("attempt {n}", { n: e.attempt + 1 })}
                </span>
              )}
              {e.http_status && (
                <span className="text-xs text-muted-foreground tabular-nums">HTTP {e.http_status}</span>
              )}
            </div>

            {e.output && (
              <div>
                <button
                  onClick={(ev) => {
                    ev.stopPropagation();
                    setExpanded(!expanded);
                  }}
                  className="flex items-center gap-1 text-xs text-muted-foreground transition-colors hover:text-foreground cursor-pointer"
                >
                  {expanded ? <ChevronUp className="h-3 w-3" /> : <ChevronDown className="h-3 w-3" />}
                  {expanded ? t("Hide output") : t("Show output")}
                </button>
                {expanded && (
                  <pre className="mt-2 rounded-md border border-border/50 bg-muted/50 p-3 text-xs text-muted-foreground font-mono max-h-48 overflow-auto">
                    {e.output}
                  </pre>
                )}
              </div>
            )}
          </div>

          <div className="text-xs text-muted-foreground text-right whitespace-nowrap leading-relaxed tabular-nums">
            <div title={fmtTime(e.started_at)}>{timeAgo(e.started_at)}</div>
            {duration && <div className="text-muted-foreground/70">{duration}</div>}
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

export default function Executions() {
  const lang = useLang();
  void lang;
  const [filterTaskId, setFilterTaskId] = useState("all");
  const [limit, setLimit] = useState(20);
  const [statusFilter, setStatusFilter] = useState("all");
  const [detailExec, setDetailExec] = useState<TaskExecution | null>(null);

  const { data, loading, error, reload } = useApi<[TaskExecution[], Task[]]>(
    () =>
      Promise.all([
        listExecutions(filterTaskId === "all" ? undefined : filterTaskId, limit),
        listTasks(),
      ]),
    [filterTaskId, limit],
  );

  useEffect(() => {
    if (error) toast.error(`${t("Failed to load executions: ")}${error.message}`);
  }, [error]);

  const [execs, tasks] = data ?? [[], []];
  // 状态过滤只作用于已加载列表(展示层),分页边界仍按原始数据计算
  const visibleExecs = statusFilter === "all" ? execs : execs.filter((e) => e.status === statusFilter);
  // 已到末尾:返回条数少于请求 limit
  const atEnd = execs.length < limit;

  return (
    <div className="space-y-6">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <h1 className="text-3xl font-bold tracking-tight">{t("Execution Logs")}</h1>
        <div className="flex flex-col gap-3 sm:flex-row sm:items-center">
          <Tabs value={statusFilter} onValueChange={setStatusFilter}>
            <TabsList>
              <TabsTrigger value="all">{t("All")}</TabsTrigger>
              <TabsTrigger value="success">{t("Success")}</TabsTrigger>
              <TabsTrigger value="failure">{t("Failure")}</TabsTrigger>
              <TabsTrigger value="running">{t("Running")}</TabsTrigger>
            </TabsList>
          </Tabs>
          <div className="flex gap-2">
          <Select value={filterTaskId} onValueChange={(v) => { setFilterTaskId(v || "all"); setLimit(20); }}>
            <SelectTrigger className="w-full sm:w-48">
              <SelectValue placeholder="All Tasks" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">{t("All Tasks")}</SelectItem>
              {tasks.map((t) => (
                <SelectItem key={t.id} value={t.id}>
                  {t.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button size="icon" variant="outline" onClick={reload} disabled={loading}>
            <RefreshCw className={`h-4 w-4 ${loading ? "animate-spin" : ""}`} />
          </Button>
          </div>
        </div>
      </div>

      {loading ? (
        <div className="space-y-3">
          {[1, 2, 3].map((i) => (
            <Card key={i} className="border-l-4 border-l-muted">
              <CardContent className="py-4">
                <div className="flex items-start justify-between gap-4">
                  <div className="flex-1 space-y-2">
                    <div className="flex items-center gap-2">
                      <Skeleton className="h-4 w-4 rounded-full" />
                      <Skeleton className="h-5 w-16 rounded-md" />
                      <Skeleton className="h-4 w-24" />
                    </div>
                    <Skeleton className="h-4 w-64" />
                  </div>
                  <Skeleton className="h-8 w-32" />
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      ) : execs.length === 0 ? (
        <div className="flex flex-col items-center justify-center rounded-xl border border-dashed py-20 text-muted-foreground">
          <span className="flex h-14 w-14 items-center justify-center rounded-full bg-primary/10 mb-4">
            <Inbox className="h-7 w-7 text-primary/60" />
          </span>
          <p className="text-sm font-medium">{t("No execution logs yet")}</p>
          <p className="text-xs mt-1 text-muted-foreground/70">{t("Tasks will appear here once they start running.")}</p>
        </div>
      ) : (
        <div className="space-y-3">
          {visibleExecs.map((e, i) => {
            const task = tasks.find((t) => t.id === e.task_id);
            return (
              <div key={e.id} style={{ animationDelay: `${Math.min(i, 12) * 40}ms` }} className="stagger-item">
                <ExecutionCard e={e} task={task} onOpen={() => setDetailExec(e)} />
              </div>
            );
          })}
          {!atEnd && (
            <div className="flex justify-center pt-2">
              <Button variant="outline" size="sm" onClick={() => setLimit((l) => l + 20)} disabled={loading}>
                <ChevronDown className="h-4 w-4" /> {t("Load More")}
              </Button>
            </div>
          )}
        </div>
      )}

      <Dialog open={detailExec !== null} onOpenChange={(open) => !open && setDetailExec(null)}>
        <DialogContent className="sm:max-w-2xl">
          {detailExec && (
            <>
              <DialogHeader>
            <DialogTitle>{t("Execution Details")}</DialogTitle>
                <DialogDescription>
                  {tasks.find((t) => t.id === detailExec.task_id)?.name ?? detailExec.task_id.slice(0, 8)}
                </DialogDescription>
              </DialogHeader>
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 text-sm">
                <div className="space-y-1">
                  <p className="text-xs text-muted-foreground">{t("Status")}</p>
                  <Badge variant={detailExec.status === "success" ? "default" : detailExec.status === "running" ? "outline" : "destructive"}>
                    {t(detailExec.status)}
                  </Badge>
                </div>
                <div className="space-y-1">
                  <p className="text-xs text-muted-foreground">{t("HTTP Status")}</p>
                  <p className="tabular-nums">{detailExec.http_status ?? "—"}</p>
                </div>
                <div className="space-y-1">
                  <p className="text-xs text-muted-foreground">{t("Started")}</p>
                  <p className="text-xs tabular-nums">{fmtTime(detailExec.started_at)}</p>
                </div>
                <div className="space-y-1">
                  <p className="text-xs text-muted-foreground">{t("Duration")}</p>
                  <p className="tabular-nums">{fmtDuration(detailExec.started_at, detailExec.finished_at) ?? "—"}</p>
                </div>
              </div>
              <div className="space-y-1.5">
                <p className="text-xs text-muted-foreground">{t("Output")}</p>
                <pre className="rounded-md border border-border/50 bg-muted/50 p-3 text-xs text-muted-foreground font-mono max-h-[50vh] overflow-auto whitespace-pre-wrap">
                  {detailExec.output ?? t("(no output)")}
                </pre>
              </div>
            </>
          )}
        </DialogContent>
      </Dialog>
    </div>
  );
}
