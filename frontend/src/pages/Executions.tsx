import { useEffect, useState, useCallback } from "react";
import { Card, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { listExecutions, listTasks, type TaskExecution, type Task } from "@/api";
import { RefreshCw, CheckCircle2, XCircle, Clock, ChevronDown, ChevronUp, Inbox } from "lucide-react";

const statusConfig: Record<string, { icon: typeof CheckCircle2; color: string; border: string }> = {
  success: { icon: CheckCircle2, color: "text-emerald-500", border: "border-l-emerald-500" },
  failure: { icon: XCircle, color: "text-red-500", border: "border-l-red-500" },
  running: { icon: Clock, color: "text-blue-500", border: "border-l-blue-500" },
};

function fmtTime(iso: string) {
  const d = new Date(iso);
  return d.toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

function fmtDuration(start: string, end: string | null) {
  if (!end) return null;
  const ms = new Date(end).getTime() - new Date(start).getTime();
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`;
  return `${Math.floor(ms / 60000)}m ${Math.round((ms % 60000) / 1000)}s`;
}

function ExecutionCard({ e, task }: { e: TaskExecution; task?: Task }) {
  const [expanded, setExpanded] = useState(false);
  const cfg = statusConfig[e.status] ?? statusConfig.failure;
  const Icon = cfg.icon;
  const duration = fmtDuration(e.started_at, e.finished_at);

  return (
    <Card className={`border-l-4 ${cfg.border} hover:shadow-sm transition-shadow`}>
      <CardContent className="py-4">
        <div className="flex items-start justify-between gap-4">
          <div className="flex-1 min-w-0 space-y-2">
            <div className="flex flex-wrap items-center gap-2">
              <Icon className={`h-4 w-4 ${cfg.color}`} />
              <Badge variant={e.status === "success" ? "default" : e.status === "running" ? "outline" : "destructive"}>
                {e.status}
              </Badge>
              {task && <span className="text-sm font-medium truncate">{task.name}</span>}
              {e.http_status && (
                <span className="text-xs text-muted-foreground tabular-nums">HTTP {e.http_status}</span>
              )}
            </div>

            {e.output && (
              <div>
                <button
                  onClick={() => setExpanded(!expanded)}
                  className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground cursor-pointer"
                >
                  {expanded ? <ChevronUp className="h-3 w-3" /> : <ChevronDown className="h-3 w-3" />}
                  {expanded ? "Hide" : "Show"} output
                </button>
                {expanded && (
                  <pre className="mt-2 text-xs text-muted-foreground bg-muted rounded-md p-3 max-h-48 overflow-auto border">
                    {e.output}
                  </pre>
                )}
              </div>
            )}
          </div>

          <div className="text-xs text-muted-foreground text-right whitespace-nowrap leading-relaxed">
            <div>{fmtTime(e.started_at)}</div>
            {duration && <div className="text-muted-foreground/70">{duration}</div>}
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

export default function Executions() {
  const [execs, setExecs] = useState<TaskExecution[]>([]);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [filterTaskId, setFilterTaskId] = useState("all");
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    setLoading(true);
    const [e, t] = await Promise.all([
      listExecutions(filterTaskId === "all" ? undefined : filterTaskId),
      listTasks(),
    ]);
    setExecs(e);
    setTasks(t);
    setLoading(false);
  }, [filterTaskId]);

  useEffect(() => {
    load();
  }, [load]);

  return (
    <div className="space-y-6">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <h1 className="text-2xl font-bold">Execution Logs</h1>
        <div className="flex gap-2">
          <Select value={filterTaskId} onValueChange={(v) => setFilterTaskId(v || "all")}>
            <SelectTrigger className="w-full sm:w-48">
              <SelectValue placeholder="All Tasks" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">All Tasks</SelectItem>
              {tasks.map((t) => (
                <SelectItem key={t.id} value={t.id}>
                  {t.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button size="icon" variant="outline" onClick={load} disabled={loading}>
            <RefreshCw className={`h-4 w-4 ${loading ? "animate-spin" : ""}`} />
          </Button>
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
        <div className="flex flex-col items-center justify-center py-16 text-muted-foreground">
          <Inbox className="h-12 w-12 mb-3 opacity-40" />
          <p className="text-sm">No execution logs yet.</p>
          <p className="text-xs mt-1">Tasks will appear here once they start running.</p>
        </div>
      ) : (
        <div className="space-y-3">
          {execs.map((e) => {
            const task = tasks.find((t) => t.id === e.task_id);
            return <ExecutionCard key={e.id} e={e} task={task} />;
          })}
        </div>
      )}
    </div>
  );
}
