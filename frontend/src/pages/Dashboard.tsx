import { useEffect, useState } from "react";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { listTasks, listExecutions, type Task, type TaskExecution } from "@/api";
import { toast } from "@/hooks/useToast";
import { ListTodo, Play, CheckCircle2, AlertCircle } from "lucide-react";

export default function Dashboard() {
  const [tasks, setTasks] = useState<Task[]>([]);
  const [execs, setExecs] = useState<TaskExecution[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    Promise.all([listTasks(), listExecutions(undefined, 20)])
      .then(([t, e]) => {
        setTasks(t);
        setExecs(e);
      })
      .catch((e) => {
        toast.error(`Failed to load dashboard: ${e instanceof Error ? e.message : String(e)}`);
      })
      .finally(() => setLoading(false));
  }, []);

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

  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-bold">Dashboard</h1>

      <div className="grid gap-4 md:grid-cols-4">
        <Card className="transition-colors hover:bg-accent/50">
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">Total Tasks</CardTitle>
            <ListTodo className="h-4 w-4 text-muted-foreground" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">{tasks.length}</div>
          </CardContent>
        </Card>

        <Card className="transition-colors hover:bg-accent/50">
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">Enabled</CardTitle>
            <Play className="h-4 w-4 text-emerald-500" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">{enabled}</div>
          </CardContent>
        </Card>

        <Card className="transition-colors hover:bg-accent/50">
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">Success</CardTitle>
            <CheckCircle2 className="h-4 w-4 text-emerald-500" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">{execs.filter((e) => e.status === "success").length}</div>
          </CardContent>
        </Card>

        <Card className="transition-colors hover:bg-accent/50">
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-sm font-medium text-muted-foreground">Failed</CardTitle>
            <AlertCircle className="h-4 w-4 text-destructive" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold">{execs.filter((e) => e.status === "failure").length}</div>
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Recent Executions</CardTitle>
        </CardHeader>
        <CardContent>
          {recent.length === 0 ? (
            <p className="text-sm text-muted-foreground">No executions yet.</p>
          ) : (
            <div className="space-y-3">
              {recent.map((e) => (
                <div key={e.id} className="flex items-center justify-between border-b pb-2 last:border-0 text-sm">
                  <div className="flex items-center gap-3">
                    <span className={`inline-flex items-center gap-1 ${e.status === "success" ? "text-emerald-600" : "text-destructive"}`}>
                      {e.status === "success" ? <CheckCircle2 className="h-4 w-4" /> : <AlertCircle className="h-4 w-4" />}
                      {e.status}
                    </span>
                    <span className="text-muted-foreground">{taskName(e.task_id)}</span>
                    {e.http_status && <span className="text-muted-foreground">HTTP {e.http_status}</span>}
                  </div>
                  <span className="text-muted-foreground">{new Date(e.started_at).toLocaleString()}</span>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
