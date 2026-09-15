# 产品演进路线图

**愿景**:自托管的"个人 crontab + 告警中心"——可靠、安全、日常顺手。

**迭代约定**:每个版本走既定流程(brainstorm → spec → plan → 逐 task 执行 → 冒烟 → 合并);feature 独立 commit;版本完成时打 git tag、更新 Cargo.toml 版本号与 CHANGELOG。

---

## v0.1.0 「可信」— 它挂了有人知道,不是谁都能动它

解决长期运行的三个致命缺口:demo → 可用产品的分水岭。

| 功能 | 设计要点 |
|------|----------|
| 失败通知 | 任务级通知配置;Generic Webhook + 飞书/钉钉内置格式;失败时与恢复时推送;payload 含任务名/错误输出(截断)/耗时/exec_id |
| API 认证 | `SCHEDULE_TOKEN` 环境变量;设置后全 API 要求 Bearer;未设置完全开放(本机模式,向后兼容);前端 401 → token 设置页(localStorage) |
| 执行记录保留 | `RETENTION_DAYS`(默认 30,0=永久);启动清理 + 每日定时清理 |

## v0.2.0 「顺手」— 日常使用无摩擦

| 功能 | 设计要点 |
|------|----------|
| 时区支持 | `chrono-tz`,任务级 `timezone` 字段(默认 UTC 兼容存量);cron 按任务时区计算;前端时区选择 |
| 失败重试 | 任务级 `max_retries` + 指数退避(30s→2m→8m);重试为独立 execution 并标记 attempt |
| next_run_at | API 返回下次执行时间;任务卡显示"2m 后执行" |
| per-task 超时 | `timeout_secs` 可配,默认 30 |
| 导入/导出 | JSON 全量导出/导入(不含执行历史) |

## v0.3.0 「可运营」— 部署运维标准化

Dockerfile + docker-compose → `/metrics`(Prometheus)+ `/healthz` → `config.toml` → 集成测试 + GitHub Actions

## v0.4.0 「轻编排」— 从定时任务到流水线

任务依赖链(A 成功触发 B,轻量非 DAG)→ Dashboard 统计图表 → i18n 中文

---

## 已完成基线(0.0.x)

- 核心调度:DelayQueue + cron/once、优雅停机、僵尸清理、cron 校验
- 执行器:HTTP(任意 method、30s 超时)+ Shell(30s 超时)、统一执行记录
- 存储:SQLite(r2d2 连接池、WAL、索引、事务删除)
- 接口:REST 全 CRUD + run + executions;CLI 全命令(错误退出码)
- 前端:暗色 SaaS 风格、Toast、确认框、状态过滤、详情弹窗、自动刷新、懒加载
