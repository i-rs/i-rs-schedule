export interface Task {
  id: string;
  name: string;
  task_type: { type: "http"; method: string; url: string; headers: unknown; body: string | null } | { type: "shell"; cmd: string };
  enabled: boolean;
  schedule: { type: "cron"; expr: string } | { type: "once"; delay_secs: number };
  timezone: string;
  timeout_secs: number;
  max_retries: number;
  max_concurrent: number;
  trigger_task_ids: string[];
  tags: string[];
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

export interface LiveSnapshot {
  version: number;
  output: string;
  total: number | null;
  done: boolean;
  status?: string;
}

/** 长轮询获取执行实时输出:cursor 为已见版本号,服务端最多 hold 25s。 */
export async function getLiveOutput(id: string, cursor: number, signal?: AbortSignal): Promise<LiveSnapshot> {
  return request(`/api/executions/${id}/live?cursor=${cursor}`, { signal });
}

export interface VarInfo {
  key: string;
  value: string | null;
  is_secret: boolean;
}

/** 全局变量(is_secret 的 value 永不返回)。 */
export async function listVars(): Promise<VarInfo[]> {
  return request("/api/vars");
}

export async function setVar(key: string, value: string, is_secret: boolean): Promise<void> {
  await request("/api/vars", { method: "POST", body: JSON.stringify({ key, value, is_secret }) });
}

export async function deleteVar(key: string): Promise<void> {
  await request(`/api/vars/${encodeURIComponent(key)}`, { method: "DELETE" });
}

/** 维护模式:暂停期间 cron 不派发、once 到期记 skipped。 */
export async function getMaintenance(): Promise<{ enabled: boolean }> {
  return request("/api/maintenance");
}

export async function setMaintenance(enabled: boolean): Promise<{ enabled: boolean }> {
  return request("/api/maintenance", { method: "POST", body: JSON.stringify({ enabled }) });
}

/** 长轮询全局事件游标:游标前进说明有执行/任务变化。 */
export async function pollEvents(cursor: number, signal?: AbortSignal): Promise<{ cursor: number }> {
  return request(`/api/events?cursor=${cursor}`, { signal });
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
  max_concurrent?: number;
  trigger_task_ids?: string[];
  tags?: string[];
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

/** 批量操作:enable | disable | delete */
export async function batchTasks(ids: string[], action: "enable" | "disable" | "delete"): Promise<{ changed: number }> {
  return request("/api/tasks/batch", { method: "POST", body: JSON.stringify({ ids, action }) });
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

export async function login(username: string, password: string): Promise<{ token: string; expires_at: string }> {
  return request<{ token: string; expires_at: string }>("/api/auth/login", {
    method: "POST",
    body: JSON.stringify({ username, password }),
  });
}

export interface ApiTokenInfo {
  id: string;
  name: string;
  created_at: string;
}

export async function listApiTokens(): Promise<ApiTokenInfo[]> {
  return request<ApiTokenInfo[]>("/api/auth/tokens");
}

export async function createApiToken(name: string): Promise<{ id: string; name: string; token: string }> {
  return request("/api/auth/tokens", { method: "POST", body: JSON.stringify({ name }) });
}

export async function revokeApiToken(id: string): Promise<void> {
  await request(`/api/auth/tokens/${id}`, { method: "DELETE" });
}

export interface AuditEntry {
  ts: string;
  action: string;
  summary: string;
}

export async function listAudit(limit = 100): Promise<AuditEntry[]> {
  return request<AuditEntry[]>(`/api/audit?limit=${limit}`);
}
