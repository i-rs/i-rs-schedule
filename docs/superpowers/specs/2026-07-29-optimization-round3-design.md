# Design: i-rs-schedule Round 3 全面优化

前后端收尾轮:后端健壮性 4 项(B1-B4),前端体验 6 项(F1-F6)。B5(Once 自动禁用)经确认不做。

## 后端

### B1 Shell 执行超时(executor.rs)
`execute_shell` 用 `tokio::time::timeout(Duration::from_secs(30), Command::output())` 包裹。超时 → `status="failure"`,output=`"shell command timed out after 30s"`。与 HTTP 30s 对齐。

### B2 delete_task 事务(db.rs)
`conn.unchecked_transaction()` 包住两条 DELETE(executions 先、task 后),`commit()` 原子生效。失败任一步整体回滚。

### B3 DB 索引(db.rs init_schema)
```sql
CREATE INDEX IF NOT EXISTS idx_task_executions_task_id ON task_executions(task_id);
CREATE INDEX IF NOT EXISTS idx_task_executions_started_at ON task_executions(started_at);
```
向后兼容,无迁移。

### B4 CLI 错误退出码(cli/main.rs)
`print_response` 解析信封:`code != 0` → stderr 输出 `error (code): message` + `exit(1)`;`code = 0` 才 pretty-print。List 子命令为 code=0 的 GET,不受影响。

## 前端

### F1 确认对话框(Tasks.tsx)
删 `window.confirm`。新增 `deleteTarget: Task | null` 状态;删除按钮设 target;复用现有 `Dialog` 组件:标题 "Delete Task"、正文显示任务名、Cancel + `variant="destructive"` Delete。确认走 `act()` 并关闭。

### F2 Dashboard 自动刷新
加载逻辑接 useApi 后,加 `setInterval(reload, 30_000)`,卸载清理。

### F3 相对时间(lib/time.ts 新建)
`timeAgo(iso)`: <60s "just now"、<60m "Nm ago"、<24h "Nh ago"、否则 "Nd ago"。Dashboard Recent 行 + Executions 卡片时间改相对时间,`title` 属性保留完整时间(hover 可查)。

### F4 路由懒加载(App.tsx)
三页面 `React.lazy` + `Suspense`,fallback 居中 spinner。bundle 按页拆分。

### F5 useApi hook(hooks/useApi.ts 新建)
`useApi<T>(fn, deps)` → `{ data, loading, error, reload, setData }`,内部 useSyncExternalStore 不需要——直接 useState/useCallback/useEffect + mounted ref。错误由页面 useEffect toast。三页面接入:
- Dashboard:`Promise.all([listTasks(), listExecutions(...)])` 双请求合一
- Tasks:`useApi(listTasks, [])`,动作后 `reload()`
- Executions:deps `[filterTaskId, limit]`,filter/limit 变更自动重载,替代手写

### F6 类型提取(api.ts)
导出 `CreateTaskPayload` 接口,`createTask`/`updateTask` 复用;Tasks.tsx payload 标注类型。

## 不改动
后端路由/响应格式/表结构(仅索引);B5;组件库;设计 token。

## 验证
- 后端:build + clippy 零 warning + fmt;冒烟:shell 超时(`sleep 60` → 30s failure)、事务删除、索引存在、CLI 400 → exit 1
- 前端:build + lint;bundle 多 chunk;DOM 验证确认框与相对时间

## 实施顺序(每步一 commit)
1. B1 shell 超时
2. B2 事务 + B3 索引
3. B4 CLI 退出码
4. F6 类型提取 + F5 useApi hook(基建)
5. F1 确认对话框(Tasks)
6. 三页接入 useApi + F2 自动刷新
7. F3 相对时间
8. F4 懒加载
9. 全量验证
