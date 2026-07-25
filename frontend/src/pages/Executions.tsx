import { useEffect, useState, useCallback } from "react";
import { Card, CardContent, Badge, Button, Select } from "@/components/ui";
import { listExecutions, listTasks, type TaskExecution, type Task } from "@/api";
import { RefreshCw, CheckCircle2, AlertCircle, Loader2 } from "lucide-react";

export default function Executions() {
  const [execs, setExecs] = useState<TaskExecution[]>([]);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [filterTaskId, setFilterTaskId] = useState("");
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    setLoading(true);
    const [e, t] = await Promise.all([
      listExecutions(filterTaskId || undefined),
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
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold">Execution Logs</h1>
        <div className="flex gap-2">
          <Select
            value={filterTaskId}
            onChange={(e) => setFilterTaskId(e.target.value)}
            placeholder="All Tasks"
            options={tasks.map((t) => ({ value: t.id, label: t.name }))}
            className="w-48"
          />
          <Button size="sm" variant="outline" onClick={load} disabled={loading}>
            <RefreshCw className="h-4 w-4" />
          </Button>
        </div>
      </div>

      {loading ? (
        <div className="flex items-center gap-2 text-muted-foreground">
          <Loader2 className="h-4 w-4 animate-spin" /> Loading…
        </div>
      ) : execs.length === 0 ? (
        <p className="text-muted-foreground">No executions yet.</p>
      ) : (
        <div className="space-y-3">
          {execs.map((e) => {
            const task = tasks.find((t) => t.id === e.task_id);
            return (
              <Card key={e.id}>
                <CardContent className="py-4">
                  <div className="flex items-start justify-between">
                    <div className="space-y-1 flex-1">
                      <div className="flex items-center gap-2">
                        <span className={`inline-flex items-center gap-1 text-sm ${e.status === "success" ? "text-emerald-600" : e.status === "running" ? "text-blue-500" : "text-destructive"}`}>
                          {e.status === "success" ? <CheckCircle2 className="h-4 w-4" /> : e.status === "running" ? <Loader2 className="h-4 w-4 animate-spin" /> : <AlertCircle className="h-4 w-4" />}
                          <Badge variant={e.status === "success" ? "default" : e.status === "running" ? "outline" : "destructive"}>{e.status}</Badge>
                        </span>
                        {task && <span className="text-sm font-medium">{task.name}</span>}
                        {e.http_status && <span className="text-xs text-muted-foreground">HTTP {e.http_status}</span>}
                      </div>
                      {e.output && (
                        <pre className="text-xs text-muted-foreground bg-muted rounded p-2 max-h-32 overflow-auto mt-1">{e.output}</pre>
                      )}
                    </div>
                    <div className="text-xs text-muted-foreground whitespace-nowrap ml-4">
                      <div>{new Date(e.started_at).toLocaleString()}</div>
                      {e.finished_at && <div>→ {new Date(e.finished_at).toLocaleString()}</div>}
                    </div>
                  </div>
                </CardContent>
              </Card>
            );
          })}
        </div>
      )}
    </div>
  );
}
