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
  "Load More": "加载更多",
  "Execution Details": "执行详情",
  Status: "状态",
  "HTTP Status": "HTTP 状态",
  Started: "开始时间",
  Duration: "耗时",
  Output: "输出",
  "(no output)": "(无输出)",
  "attempt {n}": "第 {n} 次尝试",
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
