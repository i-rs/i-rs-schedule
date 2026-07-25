import { useEffect, useState, useCallback } from "react";
import { Card, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { listExecutions, listTasks, type TaskExecution, type Task } from "@/api";
import { RefreshCw, Loader2 } from "lucide-react";

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
                <SelectItem key={t.id} value={t.id}>{t.name}</SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button size="icon" variant="outline" onClick={load} disabled={loading}>
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
                  <div className="flex flex-col sm:flex-row sm:items-start sm:justify-between gap-2">
                    <div className="space-y-1 flex-1 min-w-0">
                      <div className="flex flex-wrap items-center gap-2">
                        <Badge variant={e.status === "success" ? "default" : e.status === "running" ? "outline" : "destructive"}>{e.status}</Badge>
                        {task && <span className="text-sm font-medium">{task.name}</span>}
                        {e.http_status && <span className="text-xs text-muted-foreground">HTTP {e.http_status}</span>}
                      </div>
                      {e.output && (
                        <pre className="text-xs text-muted-foreground bg-muted rounded p-2 max-h-32 overflow-auto mt-1">{e.output}</pre>
                      )}
                    </div>
                    <div className="text-xs text-muted-foreground whitespace-nowrap">
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
