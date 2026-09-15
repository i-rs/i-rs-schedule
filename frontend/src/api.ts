export interface Task {
  id: string;
  name: string;
  task_type: { type: "http"; method: string; url: string; headers: unknown; body: string | null } | { type: "shell"; cmd: string };
  enabled: boolean;
  schedule: { type: "cron"; expr: string } | { type: "once"; delay_secs: number };
  created_at: string;
  updated_at: string;
}

export interface TaskExecution {
  id: string;
  task_id: string;
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

async function request<T>(url: string, options?: RequestInit): Promise<T> {
  const resp = await fetch(url, {
    headers: { "Content-Type": "application/json", ...options?.headers },
    ...options,
  });
  const body: ApiEnvelope<T> = await resp.json();
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
