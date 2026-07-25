import { useEffect, useState, useCallback } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from "@/components/ui/dialog";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Skeleton } from "@/components/ui/skeleton";
import { listTasks, createTask, deleteTask, enableTask, disableTask, updateTask, type Task } from "@/api";
import { Plus, Trash2, Play, Square, Pencil, RefreshCw, Globe, Terminal, Clock, Inbox } from "lucide-react";

interface TaskForm {
  name: string;
  task_type: "http" | "shell";
  schedule_type: "cron" | "once";
  cron_expr: string;
  delay_secs: string;
  http_method: string;
  http_url: string;
  http_body: string;
  shell_cmd: string;
}

const emptyForm: TaskForm = {
  name: "",
  task_type: "http",
  schedule_type: "cron",
  cron_expr: "0 */5 * * * *",
  delay_secs: "60",
  http_method: "GET",
  http_url: "",
  http_body: "",
  shell_cmd: "",
};

export default function Tasks() {
  const [tasks, setTasks] = useState<Task[]>([]);
  const [loading, setLoading] = useState(true);
  const [showDialog, setShowDialog] = useState(false);
  const [form, setForm] = useState<TaskForm>(emptyForm);
  const [editingId, setEditingId] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setTasks(await listTasks());
    setLoading(false);
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const handleSubmit = async () => {
    const payload = {
      name: form.name,
      task_type: form.task_type,
      schedule_type: form.schedule_type,
      ...(form.schedule_type === "cron" ? { cron_expr: form.cron_expr } : { delay_secs: parseInt(form.delay_secs) || 0 }),
      ...(form.task_type === "http"
        ? { http_method: form.http_method, http_url: form.http_url, http_body: form.http_body || undefined }
        : { shell_cmd: form.shell_cmd }),
    };

    if (editingId) {
      await updateTask(editingId, payload);
    } else {
      await createTask(payload);
    }
    setShowDialog(false);
    setForm(emptyForm);
    setEditingId(null);
    await load();
  };

  const openEdit = (t: Task) => {
    setEditingId(t.id);
    setForm({
      name: t.name,
      task_type: t.task_type.type,
      schedule_type: t.schedule.type,
      cron_expr: t.schedule.type === "cron" ? t.schedule.expr : "*/5 * * * *",
      delay_secs: t.schedule.type === "once" ? t.schedule.delay_secs.toString() : "60",
      http_method: t.task_type.type === "http" ? t.task_type.method : "GET",
      http_url: t.task_type.type === "http" ? t.task_type.url : "",
      http_body: t.task_type.type === "http" ? (t.task_type.body ?? "") : "",
      shell_cmd: t.task_type.type === "shell" ? t.task_type.cmd : "",
    });
    setShowDialog(true);
  };

  const openCreate = () => {
    setEditingId(null);
    setForm(emptyForm);
    setShowDialog(true);
  };

  return (
    <div className="space-y-6">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <h1 className="text-2xl font-bold">Tasks</h1>
        <div className="flex gap-2">
          <Button size="icon" variant="outline" onClick={load} disabled={loading}>
            <RefreshCw className="h-4 w-4" />
          </Button>
          <Button size="sm" onClick={openCreate}>
            <Plus className="h-4 w-4" /> New Task
          </Button>
        </div>
      </div>

      {loading ? (
        <div className="space-y-3">
          {[1, 2, 3].map((i) => (
            <Card key={i}>
              <CardContent className="flex flex-col sm:flex-row sm:items-center sm:justify-between py-4 gap-3">
                <div className="space-y-2 flex-1">
                  <div className="flex items-center gap-2">
                    <Skeleton className="h-5 w-24 rounded-md" />
                    <Skeleton className="h-5 w-16 rounded-md" />
                    <Skeleton className="h-5 w-12 rounded-md" />
                  </div>
                  <Skeleton className="h-4 w-64" />
                </div>
                <div className="flex gap-1">
                  {[1, 2, 3, 4].map((j) => (
                    <Skeleton key={j} className="h-7 w-7 rounded-md" />
                  ))}
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      ) : tasks.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-16 text-muted-foreground">
          <Inbox className="h-12 w-12 mb-3 opacity-40" />
          <p className="text-sm">No tasks yet. Create one to start scheduling.</p>
          <Button size="sm" onClick={openCreate} className="mt-3">
            <Plus className="h-4 w-4" /> Create Task
          </Button>
        </div>
      ) : (
        <div className="space-y-3">
          {tasks.map((t) => {
            const url = t.task_type.type === "http" ? t.task_type.url : t.task_type.cmd;
            const TypeIcon = t.task_type.type === "http" ? Globe : Terminal;
            return (
              <Card key={t.id} className={`border-l-4 ${t.enabled ? "border-l-emerald-500" : "border-l-muted"} hover:shadow-sm transition-shadow`}>
                <CardContent className="flex flex-col sm:flex-row sm:items-center sm:justify-between py-4 gap-3">
                  <div className="space-y-1.5 flex-1 min-w-0">
                    <div className="flex flex-wrap items-center gap-2">
                      <TypeIcon className="h-3.5 w-3.5 text-muted-foreground" />
                      <span className="font-medium text-sm">{t.name}</span>
                      <Badge variant={t.enabled ? "default" : "secondary"} className="text-[10px]">
                        {t.enabled ? "Enabled" : "Disabled"}
                      </Badge>
                      <Badge variant="outline" className="text-[10px]">{t.task_type.type.toUpperCase()}</Badge>
                      <Badge variant="outline" className="text-[10px]">
                        <Clock className="mr-1 h-3 w-3" />
                        {t.schedule.type === "cron" ? t.schedule.expr : `${t.schedule.delay_secs}s`}
                      </Badge>
                    </div>
                    <p className="text-xs text-muted-foreground truncate max-w-xl">{url}</p>
                  </div>
                  <div className="flex items-center gap-1">
                    {t.enabled ? (
                      <Button size="icon-sm" variant="ghost" title="Disable" onClick={async () => { await disableTask(t.id); load(); }}>
                        <Square className="h-4 w-4" />
                      </Button>
                    ) : (
                      <Button size="icon-sm" variant="ghost" title="Enable" onClick={async () => { await enableTask(t.id); load(); }}>
                        <Play className="h-4 w-4" />
                      </Button>
                    )}
                    <Button size="icon-sm" variant="ghost" title="Edit" onClick={() => openEdit(t)}>
                      <Pencil className="h-4 w-4" />
                    </Button>
                    <Button
                      size="icon-sm"
                      variant="ghost"
                      title="Delete"
                      onClick={async () => {
                        if (confirm("Delete this task?")) {
                          await deleteTask(t.id);
                          load();
                        }
                      }}
                    >
                      <Trash2 className="h-4 w-4 text-destructive" />
                    </Button>
                  </div>
                </CardContent>
              </Card>
            );
          })}
        </div>
      )}

      <Dialog open={showDialog} onOpenChange={setShowDialog}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>{editingId ? "Edit Task" : "Create Task"}</DialogTitle>
          </DialogHeader>

          <div className="space-y-4 py-2">
            <div>
              <Label>Name</Label>
              <Input value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} placeholder="My Task" />
            </div>

            <div>
              <Label>Type</Label>
              <Tabs value={form.task_type} onValueChange={(v) => setForm({ ...form, task_type: v as "http" | "shell" })} className="mt-1">
                <TabsList>
                  <TabsTrigger value="http">HTTP</TabsTrigger>
                  <TabsTrigger value="shell">Shell</TabsTrigger>
                </TabsList>
              </Tabs>
            </div>

            <div>
              <Label>Schedule</Label>
              <div className="flex gap-3 mt-1">
                <Select value={form.schedule_type} onValueChange={(v) => setForm({ ...form, schedule_type: v as "cron" | "once" })}>
                  <SelectTrigger className="w-40">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="cron">Cron</SelectItem>
                    <SelectItem value="once">Once (delay)</SelectItem>
                  </SelectContent>
                </Select>
                {form.schedule_type === "cron" ? (
                  <div className="space-y-2 flex-1">
                    <Input value={form.cron_expr} onChange={(e) => setForm({ ...form, cron_expr: e.target.value })} placeholder="0 */5 * * * *" />
                    <details className="text-xs text-muted-foreground">
                      <summary className="cursor-pointer hover:text-foreground">Cron reference</summary>
                      <div className="mt-2 rounded-md border bg-muted/50 p-3 space-y-2 overflow-x-auto">
                        <p className="text-muted-foreground mb-2">Format: <code className="bg-muted px-1 rounded">sec min hour dom month dow</code></p>
                        <p className="text-muted-foreground text-[11px]">6 fields required. <code>sec</code> is usually <code>0</code>.</p>
                        <table className="w-full">
                          <thead>
                            <tr className="border-b">
                              <th className="text-left py-1 pr-4">Expression</th>
                              <th className="text-left py-1">Meaning</th>
                            </tr>
                          </thead>
                          <tbody className="[&_tr]:border-b [&_tr:last-child]:border-0">
                            <tr><td className="py-1 pr-4"><code className="bg-muted px-1 rounded">0 */5 * * * *</code></td><td className="py-1">Every 5 minutes</td></tr>
                            <tr><td className="py-1 pr-4"><code className="bg-muted px-1 rounded">0 */15 * * * *</code></td><td className="py-1">Every 15 minutes</td></tr>
                            <tr><td className="py-1 pr-4"><code className="bg-muted px-1 rounded">0 * * * * *</code></td><td className="py-1">Every minute</td></tr>
                            <tr><td className="py-1 pr-4"><code className="bg-muted px-1 rounded">0 0 * * * *</code></td><td className="py-1">Every hour at :00</td></tr>
                            <tr><td className="py-1 pr-4"><code className="bg-muted px-1 rounded">0 0 0 * * *</code></td><td className="py-1">Daily at midnight</td></tr>
                            <tr><td className="py-1 pr-4"><code className="bg-muted px-1 rounded">0 0 9 * * 1-5</code></td><td className="py-1">9:00 AM, Mon–Fri</td></tr>
                            <tr><td className="py-1 pr-4"><code className="bg-muted px-1 rounded">0 0 0 1 * *</code></td><td className="py-1">Midnight on 1st of every month</td></tr>
                            <tr><td className="py-1 pr-4"><code className="bg-muted px-1 rounded">0 30 2 * * 0</code></td><td className="py-1">2:30 AM every Sunday</td></tr>
                            <tr><td className="py-1 pr-4"><code className="bg-muted px-1 rounded">0 0 0,12 * * *</code></td><td className="py-1">Midnight and noon daily</td></tr>
                          </tbody>
                        </table>
                      </div>
                    </details>
                  </div>
                ) : (
                  <Input
                    type="number"
                    value={form.delay_secs}
                    onChange={(e) => setForm({ ...form, delay_secs: e.target.value })}
                    placeholder="Seconds"
                  />
                )}
              </div>
            </div>

            {form.task_type === "http" ? (
              <>
                <div className="flex gap-3">
                  <div className="w-24">
                    <Label>Method</Label>
                    <Select value={form.http_method} onValueChange={(v) => setForm({ ...form, http_method: v || "GET" })}>
                      <SelectTrigger>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="GET">GET</SelectItem>
                        <SelectItem value="POST">POST</SelectItem>
                        <SelectItem value="PUT">PUT</SelectItem>
                        <SelectItem value="DELETE">DELETE</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                  <div className="flex-1">
                    <Label>URL</Label>
                    <Input value={form.http_url} onChange={(e) => setForm({ ...form, http_url: e.target.value })} placeholder="https://example.com/api" />
                  </div>
                </div>
                <div>
                  <Label>Body (optional)</Label>
                  <Input value={form.http_body} onChange={(e) => setForm({ ...form, http_body: e.target.value })} placeholder='{"key": "value"}' />
                </div>
              </>
            ) : (
              <div>
                <Label>Command</Label>
                <Input value={form.shell_cmd} onChange={(e) => setForm({ ...form, shell_cmd: e.target.value })} placeholder="echo hello" />
              </div>
            )}
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={() => setShowDialog(false)}>Cancel</Button>
            <Button onClick={handleSubmit} disabled={!form.name}>{editingId ? "Update" : "Create"}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
