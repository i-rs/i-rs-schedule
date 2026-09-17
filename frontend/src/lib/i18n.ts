import { useSyncExternalStore } from "react";

export type Lang = "zh" | "en";

let listeners: Array<() => void> = [];

function current(): Lang {
  const stored = localStorage.getItem("lang");
  if (stored === "zh" || stored === "en") return stored;
  return "en";
}

function emit() {
  for (const l of listeners) l();
}

export function getLang(): Lang {
  return current();
}

export function setLang(lang: Lang) {
  localStorage.setItem("lang", lang);
  emit();
}

export function toggleLang() {
  setLang(current() === "zh" ? "en" : "zh");
}

export function subscribe(listener: () => void) {
  listeners.push(listener);
  return () => {
    listeners = listeners.filter((l) => l !== listener);
  };
}

/** 响应式读取当前语言。 */
export function useLang(): Lang {
  return useSyncExternalStore(subscribe, current, current);
}

// 中文词典:key 为英文原文。
const zh: Record<string, string> = {
  // 导航 / 通用
  Dashboard: "仪表盘",
  Tasks: "任务",
  Executions: "执行记录",
  // Tasks 页
  "New Task": "新建任务",
  "No tasks yet": "还没有任务",
  "Create one to start scheduling.": "创建一个任务,开始调度。",
  "Create Task": "创建任务",
  "Edit Task": "编辑任务",
  Enabled: "已启用",
  Disabled: "已禁用",
  "Run Now": "立即运行",
  "Running...": "运行中...",
  Enable: "启用",
  Disable: "禁用",
  Edit: "编辑",
  Clone: "克隆",
  Copy: "复制",
  Delete: "删除",
  "Delete Task": "删除任务",
  Cancel: "取消",
  Create: "创建",
  Update: "更新",
  "Saving...": "保存中...",
  Name: "名称",
  Type: "类型",
  Schedule: "调度",
  "Once (delay)": "一次性(延迟)",
  Timezone: "时区",
  "Timeout (s)": "超时(秒)",
  Retries: "重试次数",
  "Cron reference": "Cron 参考",
  Method: "方法",
  "Body (optional)": "请求体(可选)",
  Command: "命令",
    "Trigger chain": "触发链",
  "On success": "成功时",
  "On failure": "失败时",
  Always: "总是",
  "Run the selected tasks when this task finishes.": "此任务结束时自动运行所选任务。",
  "On Success": "成功后",
  None: "无",
  "When this task succeeds, run the selected task automatically.":
    "此任务成功后,自动运行所选任务。",
  Notification: "通知",
  "任务失败时推送,失败后恢复会再推一条。": "任务失败时推送,失败后恢复会再推一条。",
  // 执行记录页
  "Execution Logs": "执行日志",
  All: "全部",
  Success: "成功",
  Failure: "失败",
  Failed: "失败",
  Running: "运行中",
  "All Tasks": "全部任务",
  "No execution logs yet": "暂无执行记录",
  "Tasks will appear here once they start running.":
    "任务开始运行后会显示在这里。",
  "Show output": "展开输出",
  "Hide output": "收起输出",
  "Live": "实时",
  "Skipped": "已跳过",
  "Tags": "标签",
  "Variables": "变量",
  "Secret": "密钥",
  "No variables": "暂无变量",
  "Value": "值",
  "Use {{var.key}} in shell commands and HTTP fields.": "在 shell 命令与 HTTP 字段中使用 {{var.key}} 插值。",
  "Webhook trigger": "Webhook 触发",
  "Missed-run alert": "漏跑告警",
  "Alert when a scheduled run never starts (5 min grace)": "计划执行未开始时告警(5 分钟宽限)",
  "Rotate secret": "轮换密钥",
  "Copy this URL now — the secret is shown only once.": "立即复制此 URL——密钥仅显示一次。",
  "POST to this URL to run the task. Use {{event.body}} / {{event.query.x}} in the command.": "POST 此 URL 即可触发任务;命令中可用 {{event.body}} / {{event.query.x}} 插值。",
  "Maintenance mode is on — scheduled runs are paused.": "维护模式已开启——定时调度已暂停。",
  "Resume scheduling": "恢复调度",
  "Maintenance mode enabled": "维护模式已开启",
  "Maintenance mode disabled": "维护模式已关闭",
  "Maintenance mode": "维护模式",
  "Pause all scheduled runs; manual runs are unaffected.": "暂停所有定时调度;手动运行不受影响。",
  "maintenance": "维护模式",
  "All tags": "全部标签",
  "{n} selected": "已选 {n} 项",
  "Delete {n} selected tasks?": "删除选中的 {n} 个任务?",
  "Max concurrent": "并发上限",
  "Finished": "已结束",
  "Waiting for response…": "等待响应…",
  "Load More": "加载更多",
  "Execution Details": "执行详情",
  Status: "状态",
  "HTTP Status": "HTTP 状态",
  Started: "开始时间",
  Duration: "耗时",
  Output: "输出",
  "(no output)": "(无输出)",
  "attempt {n}": "第 {n} 次尝试",
  Settings: "设置",
  "API Tokens": "API Token",
  "Audit Log": "审计日志",
  "API tokens and audit trail.": "API Token 与审计日志。",
  "Token name": "Token 名称",
  "Token created — copy it now, it will not be shown again": "Token 已创建——请立即复制,之后不再显示",
  "No tokens": "暂无 Token",
  Revoke: "吊销",
  "No audit entries": "暂无审计记录",
  "task.create": "创建任务",
  "task.update": "更新任务",
  "task.delete": "删除任务",
  "task.run": "运行任务",
  login: "登录",
  "Total runs": "总执行",
  "Avg duration": "平均耗时",
  // 仪表盘
  "Total Tasks": "任务总数",
  "Recent Executions": "最近执行",
  "No executions yet.": "暂无执行记录。",
  "Executions — last 14 days": "近 14 天执行情况",
  "No executions in the last 14 days.": "近 14 天没有执行记录。",
  // 认证 / 错误
  "Authentication required": "需要认证",
  "This server requires an API token. Paste your token to continue.":
    "此服务器需要 API Token,粘贴后继续。",
  Save: "保存",
  "Something went wrong": "出错了",
  "Try again": "重试",
  // Toast 消息
  "Task created": "任务已创建",
  "Task updated": "任务已更新",
  "Task deleted": "任务已删除",
  "Task enabled": "任务已启用",
  "Task disabled": "任务已禁用",
  "Task triggered": "任务已触发",
  "Send test notification": "发送测试通知",
  "Sending...": "发送中...",
  "Test notification delivered": "测试通知已送达",
  "Test failed": "测试失败",
  "Test notification": "测试通知",
  "Name is required": "名称必填",
  "Cron expression is required": "Cron 表达式必填",
  "URL is required for HTTP tasks": "HTTP 任务必须填写 URL",
  "URL must start with http:// or https://":
    "URL 必须以 http:// 或 https:// 开头",
  "Failed to copy": "复制失败",
  "Failed to load tasks: ": "加载任务失败:",
  "Failed to load dashboard: ": "加载仪表盘失败:",
  "Failed to load executions: ": "加载执行记录失败:",
};

/** 翻译:词典未命中时原样返回英文。 */
export function t(s: string): string {
  if (current() !== "zh") return s;
  return zh[s] ?? s;
}

/** 带参数的翻译:tf("Imported {n}", { n: 3 })。 */
export function tf(template: string, params: Record<string, string | number>): string {
  let out = t(template);
  for (const [k, v] of Object.entries(params)) {
    out = out.replaceAll(`{${k}}`, String(v));
  }
  return out;
}
