import { useEffect } from "react";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { listTasks, listExecutions, type Task, type TaskExecution } from "@/api";
import { toast } from "@/hooks/useToast";
import { useApi } from "@/hooks/useApi";
import { ListTodo, Play, CheckCircle2, AlertCircle } from "lucide-react";

export default function Dashboard() {
  const { data, loading, error, reload } = useApi<[Task[], TaskExecution[]]>(
    () => Promise.all([listTasks(), listExecutions(undefined, 20)]),
    [],
  );

  // 常驻仪表盘:每 30s 自动刷新
  useEffect(() => {
    const id = setInterval(reload, 30_000);
    return () => clearInterval(id);
  }, [reload]);

  useEffect(() => {
    if (error) toast.error(`Failed to load dashboard: ${error.message}`);
  }, [error]);

  const [tasks, execs] = data ?? [[], []];
  const enabled = tasks.filter((t) => t.enabled).length;
  const recent = execs.slice(0, 10);
  const taskName = (id: string) => tasks.find((t) => t.id === id)?.name ?? id.slice(0, 8);

  if (loading) {
    return (
      <div className="space-y-6">
        <h1 className="text-2xl font-bold">Dashboard</h1>
      <div className="grid gap-3 grid-cols-2 md:grid-cols-4">
          {[...Array(4)].map((_, i) => (
            <Skeleton key={i} className="h-24" />
          ))}
        </div>
        <Skeleton className="h-64" />
      </div>
    );
  }

  const stats = [
    { label: "Total Tasks", value: tasks.length, icon: ListTodo, color: "text-primary", glow: "bg-primary/10", bar: "bg-primary" },
    { label: "Enabled", value: enabled, icon: Play, color: "text-emerald-500", glow: "bg-emerald-500/10", bar: "bg-emerald-500" },
    { label: "Success", value: execs.filter((e) => e.status === "success").length, icon: CheckCircle2, color: "text-emerald-500", glow: "bg-emerald-500/10", bar: "bg-emerald-500" },
    { label: "Failed", value: execs.filter((e) => e.status === "failure").length, icon: AlertCircle, color: "text-destructive", glow: "bg-destructive/10", bar: "bg-destructive" },
  ];

  return (
    <div className="space-y-8">
      <h1 className="text-3xl font-bold tracking-tight">Dashboard</h1>

      <div className="grid gap-4 md:grid-cols-4">
        {stats.map((s, i) => {
          const Icon = s.icon;
          return (
            <Card
              key={s.label}
              style={{ animationDelay: `${i * 60}ms` }}
              className="stagger-item relative overflow-hidden pt-6 shadow-[var(--shadow-card)] transition-all duration-200 hover:-translate-y-0.5 hover:shadow-[var(--shadow-card-hover)]"
            >
              {/* 顶部状态色细线 */}
              <span className={`absolute inset-x-0 top-0 h-0.5 ${s.bar}`} />
              <CardHeader className="flex flex-row items-center justify-between pb-2">
                <CardTitle className="text-sm font-medium text-muted-foreground">{s.label}</CardTitle>
                <span className={`flex h-8 w-8 items-center justify-center rounded-lg ${s.glow}`}>
                  <Icon className={`h-4 w-4 ${s.color}`} />
                </span>
              </CardHeader>
              <CardContent>
                <div className="text-3xl font-semibold tabular-nums tracking-tight">{s.value}</div>
              </CardContent>
            </Card>
          );
        })}
      </div>

      <Card className="shadow-[var(--shadow-card)]">
        <CardHeader>
          <CardTitle>Recent Executions</CardTitle>
        </CardHeader>
        <CardContent>
          {recent.length === 0 ? (
            <p className="text-sm text-muted-foreground">No executions yet.</p>
          ) : (
            <div className="-mx-2">
              {recent.map((e) => {
                const ok = e.status === "success";
                return (
                  <div
                    key={e.id}
                    className="flex items-center justify-between rounded-md px-2 py-2 text-sm transition-colors hover:bg-muted/50"
                  >
                    <div className="flex items-center gap-3 min-w-0">
                      {/* 带光晕的状态点 */}
                      <span
                        className={`h-2 w-2 shrink-0 rounded-full ${ok ? "bg-emerald-500" : "bg-destructive"}`}
                        style={{ boxShadow: ok ? "0 0 8px rgba(16,185,129,0.6)" : "0 0 8px rgba(239,68,68,0.6)" }}
                      />
                      <span className={`font-medium ${ok ? "text-emerald-500" : "text-destructive"}`}>{e.status}</span>
                      <span className="text-muted-foreground truncate">{taskName(e.task_id)}</span>
                      {e.http_status && <span className="text-muted-foreground tabular-nums">HTTP {e.http_status}</span>}
                    </div>
                    <span className="text-muted-foreground tabular-nums whitespace-nowrap pl-3">
                      {new Date(e.started_at).toLocaleString()}
                    </span>
                  </div>
                );
              })}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
