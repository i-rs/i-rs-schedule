# Changelog

## v0.2.0 「顺手」(2026-09-16)

日常使用无摩擦:时区、重试、可视化调度信息、数据可迁移。

### 新增

- **时区支持**:任务级 `timezone`(IANA 名,默认 UTC);cron 按任务时区计算("9 点"就是当地 9 点);创建/更新校验非法时区;前端常用时区下拉
- **失败重试**:任务级 `max_retries`(0-10);指数退避 30s→60s→120s→…封顶 8 分钟;每次尝试独立 execution 记录(带 attempt 序号);通知只在最终结果触发
- **next_run_at**:任务 API 响应附带下次执行时间;任务卡显示 "in 5m" 徽章
- **per-task 超时**:`timeout_secs`(1-3600,默认 30)同时作用于 HTTP 与 Shell
- **导入/导出**:`GET /api/export/tasks` + `POST /api/import/tasks`(按 id 冲突跳过);前端 Export/Import 按钮

### 变更

- 任务 API 响应统一包含 `next_run_at` 计算字段

## v0.1.0 「可信」(2026-09-16)

它挂了有人知道,不是谁都能动它。三个长期运行的关键能力。

### 新增

- **失败通知**:任务级 `notify_type`(`webhook`/`feishu`/`dingtalk`)+ `notify_url`;失败时与恢复时推送(恢复 = 上次失败/中断后首次成功);payload 含任务名/状态/耗时/输出(截断 2000 字符);fire-and-forget,发送失败仅记日志
- **API 认证**:设置 `SCHEDULE_TOKEN` 后全 API 要求 `Authorization: Bearer`,否则 401;未设置完全开放(本机模式)。CLI 支持 `--token` / `SCHEDULE_TOKEN`;前端 401 时弹出 token 设置对话框(localStorage)
- **执行记录保留**:`RETENTION_DAYS`(默认 30,`0` = 永久);启动时与每 24h 清理过期 `task_executions`
- 存量数据库自动迁移(`tasks` 表补列,幂等)

### 修复

- CLI `task list` 无法解析服务端响应信封(该子命令自信封引入即不可用)
