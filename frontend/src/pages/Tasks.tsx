import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter, DialogDescription } from "@/components/ui/dialog";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Skeleton } from "@/components/ui/skeleton";
import { listTasks, createTask, deleteTask, enableTask, disableTask, updateTask, runTask, exportTasks, importTasks, type Task } from "@/api";
import { useRef } from "react";
import { toast } from "@/hooks/useToast";
import { useApi } from "@/hooks/useApi";
import { useEvents } from "@/hooks/useEvents";
import { TaskDetailDrawer } from "@/components/TaskDetailDrawer";
import { timeUntil, formatInTz } from "@/lib/time";
import { cronPreview, testNotification } from "@/api";
import { t as tr } from "@/lib/i18n";
import { Plus, Trash2, Play, Square, Pencil, RefreshCw, Globe, Terminal, Clock, Inbox, Zap, Loader2, Copy, Check, Bell, AlarmClock, Link2 } from "lucide-react";

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
  timezone: string;
  timeout_secs: string;
  max_retries: string;
  trigger_task_ids: string[];
  trigger_on: "success" | "failure" | "always";
  notify_type: "none" | "webhook" | "feishu" | "dingtalk";
  notify_url: string;
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
  timezone: "UTC",
  timeout_secs: "30",
  max_retries: "0",
  trigger_task_ids: [],
  trigger_on: "success",
  notify_type: "none",
  notify_url: "",
};

const cronPresets = [
  { label: "Every minute", expr: "0 * * * * *" },
  { label: "Every 5 min", expr: "0 */5 * * * *" },
  { label: "Every 15 min", expr: "0 */15 * * * *" },
  { label: "Hourly", expr: "0 0 * * * *" },
  { label: "Daily", expr: "0 0 0 * * *" },
  { label: "Weekdays 9am", expr: "0 0 9 * * 1-5" },
];

export default function Tasks() {
  const { data: tasksData, loading, error, reload: load } = useApi<Task[]>(listTasks, []);
  useEvents(load);
  const tasks = tasksData ?? [];
  const [showDialog, setShowDialog] = useState(false);
  const [form, setForm] = useState<TaskForm>(emptyForm);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<Task | null>(null);
  const [detailTask, setDetailTask] = useState<Task | null>(null);
  const importFileRef = useRef<HTMLInputElement>(null);
  const [runningId, setRunningId] = useState<string | null>(null);
  const [cronPreviewState, setCronPreviewState] = useState<{ times: string[] } | { error: string } | null>(null);
  const [testingNotify, setTestingNotify] = useState(false);
  const [copiedId, setCopiedId] = useState<string | null>(null);

  useEffect(() => {
    if (error) toast.error(`${tr("Failed to load tasks: ")}${error.message}`);
  }, [error]);

  // cron 实时预览:输入停顿 400ms 后请求未来 5 次触发时间
  useEffect(() => {
    if (form.schedule_type !== "cron" || !form.cron_expr.trim()) {
      setCronPreviewState(null);
      return;
    }
    const timer = setTimeout(async () => {
      try {
        const r = await cronPreview(form.cron_expr, form.timezone || "UTC");
        setCronPreviewState({ times: r.times });
      } catch (e) {
        setCronPreviewState({ error: e instanceof Error ? e.message : String(e) });
      }
    }, 400);
    return () => clearTimeout(timer);
  }, [form.cron_expr, form.timezone, form.schedule_type]);

  const handleSubmit = async () => {
    if (!form.name.trim()) {
      toast.error(tr("Name is required"));
      return;
    }
    if (form.schedule_type === "cron" && !form.cron_expr.trim()) {
      toast.error(tr("Cron expression is required"));
      return;
    }
    if (form.task_type === "http" && !form.http_url.trim()) {
      toast.error(tr("URL is required for HTTP tasks"));
      return;
    }
    if (form.task_type === "http" && !/^https?:\/\//.test(form.http_url.trim())) {
      toast.error(tr("URL must start with http:// or https://"));
      return;
    }

    const payload = {
      name: form.name,
      task_type: form.task_type,
      schedule_type: form.schedule_type,
      ...(form.schedule_type === "cron" ? { cron_expr: form.cron_expr } : { delay_secs: parseInt(form.delay_secs) || 0 }),
      ...(form.task_type === "http"
        ? { http_method: form.http_method, http_url: form.http_url, http_body: form.http_body || undefined }
        : { shell_cmd: form.shell_cmd }),
      timezone: form.timezone,
      timeout_secs: parseInt(form.timeout_secs) || 30,
      max_retries: parseInt(form.max_retries) || 0,
      trigger_task_ids: form.trigger_task_ids,
      trigger_on: form.trigger_on,
      notify_type: form.notify_type,
      ...(form.notify_type !== "none" ? { notify_url: form.notify_url } : {}),
    };

    setSubmitting(true);
    try {
      if (editingId) {
        await updateTask(editingId, payload);
        toast.success(tr("Task updated"));
      } else {
        await createTask(payload);
        toast.success(tr("Task created"));
      }
      setShowDialog(false);
      setForm(emptyForm);
      setEditingId(null);
      await load();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    } finally {
      setSubmitting(false);
    }
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
      timezone: t.timezone || "UTC",
      timeout_secs: (t.timeout_secs ?? 30).toString(),
      max_retries: (t.max_retries ?? 0).toString(),
      trigger_task_ids: t.trigger_task_ids ?? [],
      trigger_on: (t.trigger_on as TaskForm["trigger_on"]) || "success",
      notify_type: (t.notify_type as TaskForm["notify_type"]) || "none",
      notify_url: t.notify_url ?? "",
    });
    setShowDialog(true);
  };

  const openCreate = () => {
    setEditingId(null);
    setForm(emptyForm);
    setShowDialog(true);
  };

  // 包裹列表操作:失败 toast,成功后刷新。
  const act = async (fn: () => Promise<unknown>, successMsg?: string) => {
    try {
      await fn();
      if (successMsg) toast.success(tr(successMsg));
      await load();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    }
  };

  const cloneTask = (source: Task) => {
    const src = source;
    const payload = {
      name: `${src.name} (copy)`,
      task_type: src.task_type.type,
      schedule_type: src.schedule.type,
      ...(src.schedule.type === "cron"
        ? { cron_expr: src.schedule.expr }
        : { delay_secs: src.schedule.delay_secs }),
      ...(src.task_type.type === "http"
        ? { http_method: src.task_type.method, http_url: src.task_type.url, http_body: src.task_type.body ?? undefined }
        : { shell_cmd: src.task_type.cmd }),
      timezone: src.timezone,
      timeout_secs: src.timeout_secs,
      max_retries: src.max_retries,
      notify_type: src.notify_type,
      ...(src.notify_type !== "none" ? { notify_url: src.notify_url } : {}),
    };
    act(() => createTask(payload), "Task cloned");
  };

  // Run 是同步请求(等待执行完成),需要按任务粒度的 loading 反馈。
  const runTaskNow = async (id: string) => {
    setRunningId(id);
    try {
      await runTask(id);
      toast.success(tr("Task triggered"));
      await load();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    } finally {
      setRunningId(null);
    }
  };

  const copyUrl = async (t: Task) => {
    const url = t.task_type.type === "http" ? t.task_type.url : t.task_type.cmd;
    try {
      await navigator.clipboard.writeText(url);
      setCopiedId(t.id);
      setTimeout(() => setCopiedId((cur) => (cur === t.id ? null : cur)), 1500);
    } catch {
      toast.error(tr("Failed to copy"));
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <h1 className="text-3xl font-bold tracking-tight">{tr("Tasks")}</h1>
        <div className="flex gap-2">
          <input
            ref={importFileRef}
            type="file"
            accept="application/json"
            className="hidden"
            onChange={async (e) => {
              const file = e.target.files?.[0];
              if (!file) return;
              try {
                const text = await file.text();
                const payload = JSON.parse(text);
                const res = await importTasks(payload);
                toast.success(`Imported ${res.imported}, skipped ${res.skipped}`);
                await load();
              } catch (err) {
                toast.error(err instanceof Error ? err.message : String(err));
              }
              e.target.value = "";
            }}
          />
          <Button
            variant="outline"
            onClick={async () => {
              try {
                const data = await exportTasks();
                const blob = new Blob([JSON.stringify(data, null, 2)], { type: "application/json" });
                const url = URL.createObjectURL(blob);
                const a = document.createElement("a");
                a.href = url;
                a.download = `i-rs-schedule-tasks-${new Date().toISOString().slice(0, 10)}.json`;
                a.click();
                URL.revokeObjectURL(url);
              } catch (err) {
                toast.error(err instanceof Error ? err.message : String(err));
              }
            }}
          >
            Export
          </Button>
          <Button variant="outline" onClick={() => importFileRef.current?.click()}>
            Import
          </Button>
          <Button size="icon" variant="outline" onClick={load} disabled={loading}>
            <RefreshCw className="h-4 w-4" />
          </Button>
          <Button size="sm" onClick={openCreate}>
            <Plus className="h-4 w-4" /> {tr("New Task")}
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
        <div className="flex flex-col items-center justify-center rounded-xl border border-dashed py-20 text-muted-foreground">
          <span className="flex h-14 w-14 items-center justify-center rounded-full bg-primary/10 mb-4">
            <Inbox className="h-7 w-7 text-primary/60" />
          </span>
          <p className="text-sm font-medium">{tr("No tasks yet")}</p>
          <p className="text-xs mt-1 text-muted-foreground/70">{tr("Create one to start scheduling.")}</p>
          <Button size="sm" onClick={openCreate} className="mt-4">
            <Plus className="h-4 w-4" /> {tr("Create Task")}
          </Button>
        </div>
      ) : (
        <div className="space-y-3">
          {tasks.map((t, i) => {
            const url = t.task_type.type === "http" ? t.task_type.url : t.task_type.cmd;
            const TypeIcon = t.task_type.type === "http" ? Globe : Terminal;
            return (
              <Card
                key={t.id}
                onClick={() => setDetailTask(t)}
                style={{ animationDelay: `${Math.min(i, 12) * 40}ms` }}
                className={`group/task stagger-item cursor-pointer border-l-4 shadow-[var(--shadow-card)] transition-all duration-200 hover:shadow-[var(--shadow-card-hover)] ${t.enabled ? "border-l-primary" : "border-l-muted-foreground/40"}`}
              >
                <CardContent className="flex flex-col sm:flex-row sm:items-center sm:justify-between py-4 gap-3">
                  <div className="space-y-1.5 flex-1 min-w-0">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className={`flex h-6 w-6 items-center justify-center rounded-md ${t.task_type.type === "http" ? "bg-primary/10 text-primary" : "bg-violet-500/10 text-violet-500"}`}>
                        <TypeIcon className="h-3.5 w-3.5" />
                      </span>
                      <span className="font-medium text-[15px]">{t.name}</span>
                      <Badge variant={t.enabled ? "default" : "secondary"} className="text-[10px]">
                        {t.enabled ? tr("Enabled") : tr("Disabled")}
                      </Badge>
                      <Badge variant="outline" className="text-[10px]">{t.task_type.type.toUpperCase()}</Badge>
                      <Badge variant="outline" className="text-[10px]">
                        <Clock className="mr-1 h-3 w-3" />
                        {t.schedule.type === "cron" ? t.schedule.expr : `${t.schedule.delay_secs}s`}
                      </Badge>
                      {t.next_run_at && (
                        <Badge variant="outline" className="text-[10px]">
                          <AlarmClock className="mr-1 h-3 w-3" />
                          {timeUntil(t.next_run_at)}
                        </Badge>
                      )}
                      {t.notify_type !== "none" && (
                        <Bell className="h-3 w-3 text-amber-400" />
                      )}
                      {t.trigger_task_ids.length > 0 && (
                        <Link2 className="h-3 w-3 text-sky-400" />
                      )}
                    </div>
                    <div className="flex items-center gap-1.5 min-w-0">
                      <p className="text-xs text-muted-foreground truncate font-mono">{url}</p>
                      <button
                        title={tr("Copy")}
                        onClick={() => copyUrl(t)}
                        className="shrink-0 text-muted-foreground/50 hover:text-foreground transition-all cursor-pointer"
                      >
                        {copiedId === t.id ? <Check className="h-3 w-3 text-emerald-500" /> : <Copy className="h-3 w-3" />}
                      </button>
                    </div>
                  </div>
                  <div
                    className="flex items-center gap-1 opacity-60 transition-opacity duration-200 group-hover/task:opacity-100"
                    onClick={(e) => e.stopPropagation()}
                  >
                    {t.enabled ? (
                      <Button size="icon-sm" variant="ghost" title="Disable" onClick={() => act(() => disableTask(t.id), "Task disabled")}>
                        <Square className="h-4 w-4" />
                      </Button>
                    ) : (
                      <Button size="icon-sm" variant="ghost" title="Enable" onClick={() => act(() => enableTask(t.id), "Task enabled")}>
                        <Play className="h-4 w-4" />
                      </Button>
                    )}
                    <Button
                      size="icon-sm"
                      variant="ghost"
                      title={runningId === t.id ? tr("Running...") : tr("Run Now")}
                      disabled={runningId === t.id}
                      onClick={() => runTaskNow(t.id)}
                    >
                      {runningId === t.id ? <Loader2 className="h-4 w-4 animate-spin" /> : <Zap className="h-4 w-4" />}
                    </Button>
                    <Button size="icon-sm" variant="ghost" title={tr("Edit")} onClick={() => openEdit(t)}>
                      <Pencil className="h-4 w-4" />
                    </Button>
                    <Button size="icon-sm" variant="ghost" title={tr("Clone")} onClick={() => cloneTask(t)}>
                      <Copy className="h-4 w-4" />
                    </Button>
                    <Button
                      size="icon-sm"
                      variant="ghost"
                      title={tr("Delete")}
                      onClick={() => setDeleteTarget(t)}
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
        <DialogContent className="sm:max-w-xl">
          <DialogHeader>
            <DialogTitle>{editingId ? tr("Edit Task") : tr("Create Task")}</DialogTitle>
          </DialogHeader>

          <div className="space-y-4 py-2">
            <div className="space-y-1.5">
              <Label>{tr("Name")}</Label>
              <Input value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} placeholder="My Task" />
            </div>

            <div className="space-y-1.5">
              <Label>{tr("Type")}</Label>
              <Tabs value={form.task_type} onValueChange={(v) => setForm({ ...form, task_type: v as "http" | "shell" })}>
                <TabsList>
                  <TabsTrigger value="http">HTTP</TabsTrigger>
                  <TabsTrigger value="shell">Shell</TabsTrigger>
                </TabsList>
              </Tabs>
            </div>

            <div className="space-y-1.5">
              <Label>{tr("Schedule")}</Label>
              <div className="flex gap-3">
                <Select value={form.schedule_type} onValueChange={(v) => setForm({ ...form, schedule_type: v as "cron" | "once" })}>
                  <SelectTrigger className="w-40">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="cron">Cron</SelectItem>
                    <SelectItem value="once">{tr("Once (delay)")}</SelectItem>
                  </SelectContent>
                </Select>
                {form.schedule_type === "cron" ? (
                  <div className="space-y-2 flex-1">
                    <Input value={form.cron_expr} onChange={(e) => setForm({ ...form, cron_expr: e.target.value })} placeholder="0 */5 * * * *" className="font-mono" />
                    <div className="flex flex-wrap gap-1.5">
                      {cronPresets.map((p) => (
                        <button
                          key={p.expr}
                          type="button"
                          onClick={() => setForm({ ...form, cron_expr: p.expr })}
                          className={`rounded-full border px-2 py-0.5 text-[11px] transition-colors cursor-pointer ${
                            form.cron_expr === p.expr
                              ? "border-primary bg-primary/10 text-primary"
                              : "border-border text-muted-foreground hover:border-primary/50 hover:text-foreground"
                          }`}
                        >
                          {p.label}
                        </button>
                      ))}
                    </div>
                    <div className="flex items-center gap-3">
                      <div className="flex items-center gap-1.5">
                        <Label className="text-xs text-muted-foreground">{tr("Timeout (s)")}</Label>
                        <Input
                          type="number"
                          value={form.timeout_secs}
                          onChange={(e) => setForm({ ...form, timeout_secs: e.target.value })}
                          className="w-24 h-7 text-xs"
                        />
                      </div>
                      <div className="flex items-center gap-1.5">
                        <Label className="text-xs text-muted-foreground">{tr("Retries")}</Label>
                        <Input
                          type="number"
                          value={form.max_retries}
                          onChange={(e) => setForm({ ...form, max_retries: e.target.value })}
                          className="w-24 h-7 text-xs"
                        />
                      </div>
                    </div>
                    <div className="flex items-center gap-1.5">
                      <Label className="text-xs text-muted-foreground">{tr("Timezone")}</Label>
                      <Select value={form.timezone} onValueChange={(v) => setForm({ ...form, timezone: v || "UTC" })}>
                        <SelectTrigger className="w-44 h-7 text-xs">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {["UTC","Asia/Shanghai","Asia/Hong_Kong","Asia/Tokyo","Asia/Singapore","Europe/London","Europe/Berlin","America/New_York","America/Los_Angeles"].map((z) => (
                            <SelectItem key={z} value={z}>{z}</SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </div>
                    {cronPreviewState && "times" in cronPreviewState && (
                      <div className="text-xs text-muted-foreground space-y-0.5 tabular-nums">
                        {cronPreviewState.times.map((iso) => (
                          <div key={iso} className="flex items-center gap-1.5">
                            <span className="text-emerald-500">▸</span>
                            {formatInTz(iso, form.timezone || "UTC")}
                          </div>
                        ))}
                      </div>
                    )}
                    {cronPreviewState && "error" in cronPreviewState && (
                      <p className="text-xs text-destructive">{cronPreviewState.error}</p>
                    )}
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
                  <div className="w-24 space-y-1.5">
                    <Label>{tr("Method")}</Label>
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
                  <div className="flex-1 space-y-1.5">
                    <Label>{tr("URL")}</Label>
                    <Input value={form.http_url} onChange={(e) => setForm({ ...form, http_url: e.target.value })} placeholder="https://example.com/api" />
                  </div>
                </div>
                <div className="space-y-1.5">
                  <Label>{tr("Body (optional)")}</Label>
                  <Textarea
                    value={form.http_body}
                    onChange={(e) => setForm({ ...form, http_body: e.target.value })}
                    placeholder={'{\n  "key": "value"\n}'}
                    className="font-mono min-h-[80px]"
                  />
                </div>
              </>
            ) : (
              <div className="space-y-1.5">
                <Label>{tr("Command")}</Label>
                <Textarea
                  value={form.shell_cmd}
                  onChange={(e) => setForm({ ...form, shell_cmd: e.target.value })}
                  placeholder="echo hello"
                  className="font-mono min-h-[64px]"
                  rows={2}
                />
              </div>
            )}

            <div className="space-y-1.5">
              <Label>{tr("Trigger chain")}</Label>
              <Select value={form.trigger_on} onValueChange={(v) => setForm({ ...form, trigger_on: v as TaskForm["trigger_on"] })}>
                <SelectTrigger className="w-40">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="success">{tr("On success")}</SelectItem>
                  <SelectItem value="failure">{tr("On failure")}</SelectItem>
                  <SelectItem value="always">{tr("Always")}</SelectItem>
                </SelectContent>
              </Select>
              {tasks.filter((t) => t.id !== editingId).length > 0 && (
                <div className="rounded-lg border border-border/50 p-2.5 space-y-1.5 max-h-36 overflow-y-auto">
                  {tasks.filter((t) => t.id !== editingId).map((t) => (
                    <label key={t.id} className="flex items-center gap-2 text-sm cursor-pointer">
                      <input
                        type="checkbox"
                        checked={form.trigger_task_ids.includes(t.id)}
                        onChange={(e) => {
                          setForm((f) => ({
                            ...f,
                            trigger_task_ids: e.target.checked
                              ? [...f.trigger_task_ids, t.id]
                              : f.trigger_task_ids.filter((id) => id !== t.id),
                          }));
                        }}
                        className="accent-primary"
                      />
                      <span className="truncate">{t.name}</span>
                    </label>
                  ))}
                </div>
              )}
              <p className="text-xs text-muted-foreground">
                {tr("Run the selected tasks when this task finishes.")}
              </p>
            </div>

            <div className="space-y-1.5">
              <Label>{tr("Notification")}</Label>
              <div className="flex gap-3">
                <Select value={form.notify_type} onValueChange={(v) => setForm({ ...form, notify_type: v as TaskForm["notify_type"] })}>
                  <SelectTrigger className="w-32">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="none">{tr("None")}</SelectItem>
                    <SelectItem value="webhook">Webhook</SelectItem>
                    <SelectItem value="feishu">飞书</SelectItem>
                    <SelectItem value="dingtalk">钉钉</SelectItem>
                  </SelectContent>
                </Select>
                {form.notify_type !== "none" && (
                  <Input
                    value={form.notify_url}
                    onChange={(e) => setForm({ ...form, notify_url: e.target.value })}
                    placeholder="https://open.feishu.cn/open-apis/bot/v2/hook/..."
                    className="flex-1 font-mono"
                  />
                )}
              </div>
              {editingId && form.notify_type !== "none" && (
                <div>
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={testingNotify}
                    onClick={async () => {
                      if (!editingId) return;
                      setTestingNotify(true);
                      try {
                        const r = await testNotification(editingId);
                        if (r.delivered) toast.success(tr("Test notification delivered"));
                        else toast.error(`${tr("Test failed")}: ${r.detail}`);
                      } catch (e) {
                        toast.error(e instanceof Error ? e.message : String(e));
                      } finally {
                        setTestingNotify(false);
                      }
                    }}
                  >
                    {testingNotify ? tr("Sending...") : tr("Send test notification")}
                  </Button>
                </div>
              )}
              {form.notify_type !== "none" && (
                <p className="text-xs text-muted-foreground">
                  任务失败时推送,失败后恢复会再推一条。
                </p>
              )}
            </div>
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={() => setShowDialog(false)} disabled={submitting}>{tr("Cancel")}</Button>
            <Button onClick={handleSubmit} disabled={!form.name || submitting}>
              {submitting ? tr("Saving...") : editingId ? tr("Update") : tr("Create")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <TaskDetailDrawer task={detailTask} onClose={() => setDetailTask(null)} />

      <Dialog open={deleteTarget !== null} onOpenChange={(open) => !open && setDeleteTarget(null)}>
        <DialogContent className="sm:max-w-sm" showCloseButton={false}>
          <DialogHeader>
            <DialogTitle>{tr("Delete Task")}</DialogTitle>
            <DialogDescription>
              Delete <span className="font-medium text-foreground">{deleteTarget?.name}</span>? Its
              execution history will be removed as well. This cannot be undone.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteTarget(null)}>{tr("Cancel")}</Button>
            <Button
              variant="destructive"
              onClick={() => {
                if (deleteTarget) {
                  act(() => deleteTask(deleteTarget.id), "Task deleted");
                }
                setDeleteTarget(null);
              }}
            >
              {tr("Delete")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
