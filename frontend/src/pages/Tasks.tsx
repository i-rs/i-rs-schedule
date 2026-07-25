import { useEffect, useState, useCallback } from "react";
import { Button, Card, CardContent, Input, Label, Select, Badge, Dialog, Tabs, TabsList, TabsTrigger } from "@/components/ui";
import { listTasks, createTask, deleteTask, enableTask, disableTask, updateTask, type Task } from "@/api";
import { Plus, Trash2, Play, Square, Pencil, RefreshCw } from "lucide-react";

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
  cron_expr: "*/5 * * * *",
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
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold">Tasks</h1>
        <div className="flex gap-2">
          <Button size="sm" variant="outline" onClick={load} disabled={loading}>
            <RefreshCw className="h-4 w-4" />
          </Button>
          <Button size="sm" onClick={openCreate}>
            <Plus className="h-4 w-4" /> New Task
          </Button>
        </div>
      </div>

      {loading ? (
        <p className="text-muted-foreground">Loading…</p>
      ) : tasks.length === 0 ? (
        <p className="text-muted-foreground">No tasks yet. Create one to get started.</p>
      ) : (
        <div className="space-y-3">
          {tasks.map((t) => {
            const url = t.task_type.type === "http" ? t.task_type.url : t.task_type.cmd;
            return (
              <Card key={t.id}>
                <CardContent className="flex items-center justify-between py-4">
                  <div className="space-y-1">
                    <div className="flex items-center gap-2">
                      <span className="font-medium">{t.name}</span>
                      <Badge variant={t.enabled ? "default" : "secondary"}>{t.enabled ? "Enabled" : "Disabled"}</Badge>
                      <Badge variant="outline">{t.task_type.type.toUpperCase()}</Badge>
                      <Badge variant="outline">{t.schedule.type === "cron" ? t.schedule.expr : `${t.schedule.delay_secs}s`}</Badge>
                    </div>
                    <p className="text-sm text-muted-foreground truncate max-w-xl">{url}</p>
                  </div>
                  <div className="flex items-center gap-1">
                    {t.enabled ? (
                      <Button size="icon" variant="ghost" title="Disable" onClick={async () => { await disableTask(t.id); load(); }}>
                        <Square className="h-4 w-4" />
                      </Button>
                    ) : (
                      <Button size="icon" variant="ghost" title="Enable" onClick={async () => { await enableTask(t.id); load(); }}>
                        <Play className="h-4 w-4" />
                      </Button>
                    )}
                    <Button size="icon" variant="ghost" title="Edit" onClick={() => openEdit(t)}>
                      <Pencil className="h-4 w-4" />
                    </Button>
                    <Button
                      size="icon"
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

      <Dialog open={showDialog} onClose={() => setShowDialog(false)} title={editingId ? "Edit Task" : "Create Task"}>
        <div className="space-y-4">
          <div>
            <Label>Name</Label>
            <Input value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} placeholder="My Task" />
          </div>

          <Tabs
            defaultValue={form.task_type}
            className="space-y-3"
          >
            <TabsList>
              <TabsTrigger value="http" active={form.task_type} setActive={(v) => setForm({ ...form, task_type: v as "http" | "shell" })}>HTTP</TabsTrigger>
              <TabsTrigger value="shell" active={form.task_type} setActive={(v) => setForm({ ...form, task_type: v as "http" | "shell" })}>Shell</TabsTrigger>
            </TabsList>
          </Tabs>

          <div>
            <Label>Schedule</Label>
            <div className="flex gap-3 mt-1">
              <Select
                value={form.schedule_type}
                onChange={(e) => setForm({ ...form, schedule_type: e.target.value as "cron" | "once" })}
                options={[
                  { value: "cron", label: "Cron" },
                  { value: "once", label: "Once (delay)" },
                ]}
              />
              {form.schedule_type === "cron" ? (
                <Input value={form.cron_expr} onChange={(e) => setForm({ ...form, cron_expr: e.target.value })} placeholder="*/5 * * * *" />
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
                  <Select
                    value={form.http_method}
                    onChange={(e) => setForm({ ...form, http_method: e.target.value })}
                    options={[
                      { value: "GET", label: "GET" },
                      { value: "POST", label: "POST" },
                      { value: "PUT", label: "PUT" },
                      { value: "DELETE", label: "DELETE" },
                    ]}
                  />
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

          <div className="flex justify-end gap-2 pt-2">
            <Button variant="outline" onClick={() => setShowDialog(false)}>Cancel</Button>
            <Button onClick={handleSubmit} disabled={!form.name}>{editingId ? "Update" : "Create"}</Button>
          </div>
        </div>
      </Dialog>
    </div>
  );
}
