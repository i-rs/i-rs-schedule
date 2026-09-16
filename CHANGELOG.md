# Changelog

## v0.4.0 「轻编排」(2026-09-16)

从定时任务到流水线,界面走向双语。

### 新增

- **任务依赖链**:任务级 `trigger_task_id`——本任务成功后自动运行下游任务;保存时校验自环/循环(沿链最多 20 步)/目标存在;运行时深度上限 10;链式任务走完整重试与通知逻辑
- **Dashboard 统计图表**:近 14 天执行堆叠柱状图(成功/失败),shadcn chart + Recharts 实现,hover 显示当日详情
- **双语界面(zh/en)**:header 语言切换按钮,偏好存 localStorage,默认英文;覆盖导航、页面、表单、对话框、徽章、toast、相对时间格式

### 修复

- CLI `--max-retries` 从未生效(flag 在 v0.2 遗漏),本版补齐

## v0.3.0 「可运营」(2026-09-16)

部署与运维标准化。

### 新增

- **健康检查与指标**:`GET /healthz` 探活;`GET /metrics` 输出 Prometheus 文本格式(执行总数/成功/失败计数、启用任务 gauge)
- **Docker 部署**:多阶段 Dockerfile + docker-compose(数据卷、healthcheck、token 示例)
- **CI**:GitHub Actions(cargo fmt/clippy -D warnings/test/build + 前端 lint/build)
- **config.toml**:可选配置文件(db_path/port/token/retention_days),优先级 环境变量 > 文件 > 默认值
- **测试套件**:9 个单元测试(时区数学、cron/时区校验、next_run_at 语义、config 解析),`cargo test` 进 CI

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
