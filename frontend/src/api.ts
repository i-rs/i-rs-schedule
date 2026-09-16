export interface Task {
  id: string;
  name: string;
  task_type: { type: "http"; method: string; url: string; headers: unknown; body: string | null } | { type: "shell"; cmd: string };
  enabled: boolean;
  schedule: { type: "cron"; expr: string } | { type: "once"; delay_secs: number };
  timezone: string;
  timeout_secs: number;
  max_retries: number;
  trigger_task_ids: string[];
  trigger_on: string;
  next_run_at: string | null;
  notify_type: string;
  notify_url: string;
  created_at: string;
  updated_at: string;
}

export interface TaskExecution {
  id: string;
  task_id: string;
  attempt: number;
  status: string;
  output: string | null;
  http_status: number | null;
  started_at: string;
  finished_at: string | null;
}

interface ApiEnvelope<T> {
  code: number;
  message: string;
  data: T;
}

export function getToken(): string | null {
  return localStorage.getItem("schedule_token");
}

export function setToken(token: string) {
  if (token) localStorage.setItem("schedule_token", token);
  else localStorage.removeItem("schedule_token");
}

async function request<T>(url: string, options?: RequestInit): Promise<T> {
  const token = getToken();
  const resp = await fetch(url, {
    headers: {
      "Content-Type": "application/json",
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      ...options?.headers,
    },
    ...options,
  });
  const body: ApiEnvelope<T> = await resp.json();
  if (body.code === 401) {
    window.dispatchEvent(new CustomEvent("auth-required"));
    throw new Error("Unauthorized");
  }
  if (body.code !== 0) {
    throw new Error(body.message);
  }
  return body.data;
}

export async function listTasks(): Promise<Task[]> {
  return request<Task[]>("/api/tasks");
}

export async function getTask(id: string): Promise<Task> {
  return request<Task>(`/api/tasks/${id}`);
}

export interface CreateTaskPayload {
  name: string;
  task_type: string;
  schedule_type: string;
  cron_expr?: string;
  delay_secs?: number;
  http_method?: string;
  http_url?: string;
  http_headers?: unknown;
  http_body?: string;
  shell_cmd?: string;
  timezone?: string;
  timeout_secs?: number;
  max_retries?: number;
  trigger_task_ids?: string[];
  trigger_on?: string;
  notify_type?: string;
  notify_url?: string;
}

export async function createTask(data: CreateTaskPayload): Promise<Task> {
  return request<Task>("/api/tasks", {
    method: "POST",
    body: JSON.stringify(data),
  });
}

export async function updateTask(id: string, data: CreateTaskPayload): Promise<Task> {
  return request<Task>(`/api/tasks/${id}`, {
    method: "PUT",
    body: JSON.stringify(data),
  });
}

export async function deleteTask(id: string): Promise<void> {
  await request(`/api/tasks/${id}`, { method: "DELETE" });
}

export async function runTask(id: string): Promise<TaskExecution> {
  return request<TaskExecution>(`/api/tasks/${id}/run`, { method: "POST" });
}

export async function enableTask(id: string): Promise<void> {
  await request(`/api/tasks/${id}/enable`, { method: "POST" });
}

export async function disableTask(id: string): Promise<void> {
  await request(`/api/tasks/${id}/disable`, { method: "POST" });
}

export async function listExecutions(taskId?: string, limit = 50): Promise<TaskExecution[]> {
  const params = new URLSearchParams();
  if (taskId) params.set("task_id", taskId);
  params.set("limit", limit.toString());
  return request<TaskExecution[]>(`/api/executions?${params}`);
}

export async function exportTasks(): Promise<{ version: string; tasks: Task[] }> {
  return request("/api/export/tasks");
}

export async function importTasks(payload: { version: string; tasks: Task[] }): Promise<{ imported: number; skipped: number }> {
  return request("/api/import/tasks", {
    method: "POST",
    body: JSON.stringify(payload),
  });
}

export interface DailyStat {
  date: string;
  success: number;
  failure: number;
  avg_duration_ms: number | null;
}

export async function dailyStats(): Promise<DailyStat[]> {
  return request<DailyStat[]>("/api/stats/daily");
}

export interface TaskStats {
  total: number;
  success: number;
  failure: number;
  avg_duration_ms: number | null;
}

export async function getTaskStats(id: string): Promise<TaskStats> {
  return request<TaskStats>(`/api/tasks/${id}/stats`);
}

export async function listTaskExecutions(id: string, limit = 20): Promise<TaskExecution[]> {
  return request<TaskExecution[]>(`/api/executions?task_id=${id}&limit=${limit}`);
}

export async function cronPreview(expr: string, timezone: string): Promise<{ times: string[] }> {
  return request<{ times: string[] }>("/api/cron/preview", {
    method: "POST",
    body: JSON.stringify({ expr, timezone }),
  });
}

export async function testNotification(id: string): Promise<{ delivered: boolean; detail: string }> {
  return request<{ delivered: boolean; detail: string }>(`/api/tasks/${id}/notify-test`, {
    method: "POST",
  });
}
