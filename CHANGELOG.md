# Changelog

## v0.1.0 「可信」(2026-09-16)

它挂了有人知道,不是谁都能动它。三个长期运行的关键能力。

### 新增

- **失败通知**:任务级 `notify_type`(`webhook`/`feishu`/`dingtalk`)+ `notify_url`;失败时与恢复时推送(恢复 = 上次失败/中断后首次成功);payload 含任务名/状态/耗时/输出(截断 2000 字符);fire-and-forget,发送失败仅记日志
- **API 认证**:设置 `SCHEDULE_TOKEN` 后全 API 要求 `Authorization: Bearer`,否则 401;未设置完全开放(本机模式)。CLI 支持 `--token` / `SCHEDULE_TOKEN`;前端 401 时弹出 token 设置对话框(localStorage)
- **执行记录保留**:`RETENTION_DAYS`(默认 30,`0` = 永久);启动时与每 24h 清理过期 `task_executions`
- 存量数据库自动迁移(`tasks` 表补列,幂等)

### 修复

- CLI `task list` 无法解析服务端响应信封(该子命令自信封引入即不可用)
