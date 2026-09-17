# Changelog

## v0.10.0 「事件」(2026-09-18)

不止定时:外部世界也能触发任务,沉默故障也有人知道。

### 新增

- **Webhook 触发器**:`POST /api/tasks/:id/hook` 开启/轮换(secret 明文仅返回一次,sha256 入库)、`DELETE` 关闭;触发端点 `POST /api/hooks/:task_id/:secret` 命中即后台执行(secret 即凭据,豁免 Bearer 认证;常数时间比较);支持 `{{event.body}}` / `{{event.query.x}}` 插值;复用并发互斥/重试/通知/审计管线。任务抽屉管理(开启→URL 复制一次→轮换/关闭);CLI `task hook <id> enable|disable`
- **变量与密钥**:全局变量 `variables` 表;shell cmd 与 HTTP url/body/header 值支持 `{{var.key}}` 插值(执行时加载);`GET/POST/DELETE /api/vars`;is_secret 的 value 永不回传前端、导出不含变量;设置对话框 Variables 标签页;CLI `var list|set|delete`
- **漏跑检测(deadman)**:任务 `missed_alert` 开关——期望时间(cron 上一触发点)过后 5 分钟宽限期内无任何执行尝试 → 按任务通知渠道推送 `task_missed` 告警;同一期望时间只告警一次;每 60s 扫描,查询失败宁可漏报不误报。表单"漏跑告警"复选框;CLI `--missed-alert`

### 变更

- `tasks` 表新增 `hook_secret_hash`、`missed_alert` 列(幂等迁移);新增 `variables` 表
- 范围裁剪:任务级变量不做(与直接写值等价的假抽象);`{{event.header.x}}` 暂不支持

## v0.9.0 「秩序」(2026-09-17)

任务多了、跑久了,不失控:并发互斥、标签组织、维护模式、自动备份。

### 新增

- **并发与互斥**:任务级 `max_concurrent`(默认 1,0=不限并行)——上轮未结束即跳过并落库 `skipped` 记录(防重复扣款/通知);重试退避期间占住槽位;全局并发兜底 `GLOBAL_MAX_CONCURRENCY`(默认 32,超限等待)。前端 Skipped 琥珀徽章与过滤;CLI `task add/update --max-concurrent`
- **标签系统**:任务多标签(`tags` 列,JSON 数组);列表按标签过滤、卡片复选框多选 → 批量启用/禁用/删除(`POST /api/tasks/batch`);标签按名称确定性配色;抽屉展示标签;CLI `--tags "a,b"`
- **维护模式**:一键全局暂停定时调度——cron 到期跳到下次(错过不补发)、once 到期记 `missed during maintenance`;在途执行不中断;手动 run 不受影响;重启后状态恢复;`GET/POST /api/maintenance`;顶栏琥珀横幅 + 设置对话框开关;CLI `maintenance status|on|off`;`/healthz` 附带状态
- **自动备份**:`BACKUP_DIR`(未配置=关闭)+ `BACKUP_KEEP`(默认 7)——启动 + 每日 `VACUUM INTO` 在线备份,超出保留份数自动清理;`/healthz` 附带 `last_backup`

### 变更

- `tasks` 表新增 `max_concurrent`、`tags` 列(幂等迁移);新增 `settings` 表
- 排队模式推迟:重叠执行先靠 skip 兜底,真实需求应调整超时/调度间隔

## v0.8.0 「实时」(2026-09-17)

执行不再是黑盒:实时输出流、输出上限、推送式刷新、可安装 PWA。

### 新增

- **实时输出流**:执行中任务的输出实时上屏(长轮询,`GET /api/executions/:id/live?cursor=`);任务抽屉与执行详情内嵌终端风格实时视图(光标闪烁、字节数、自动滚动);完成后与落库内容一致
- **输出持久化上限**:`MAX_OUTPUT_KB`(config.toml `max_output_kb`,默认 64,0=不限)——超限保留头部+尾部+截断标记,内存与落库占用有界
- **事件驱动刷新**:`GET /api/events?cursor=` 长轮询,任何执行/任务变化推进游标;Tasks / Executions / Dashboard / 抽屉全部改为事件触发刷新(断线自动退避重连),Dashboard 30s 轮询退役
- **PWA**:manifest + 图标 + 极简 service worker(静态资源 cache-first、壳层 network-first、`/api/*` 永不缓存),可安装到手机桌面

### 变更

- Shell 执行从 `.output()` 改为 spawn + stdout/stderr 双路增量读取:输出按到达顺序**合并持久化**(此前成功只存 stdout、失败只存 stderr);`{{trigger.output}}` 拿到合并输出
- HTTP 响应体改为 `bytes()` + 有界累积;超时行为不变(杀进程、保留已产出输出并追加超时提示)
- 技术说明:desirable 响应体为 `Full<Bytes>`,不支持流式 SSE,实时流以 25s 长轮询实现(复用现有 Bearer 认证,体验等价)

## v0.7.0 「多人」(2026-09-16)

从个人工具走向团队工具:登录、Token 管理、审计、全局通知。

### 新增

- **登录会话**:配置 `ADMIN_USER`/`ADMIN_PASSWORD`(或 config.toml 同名字段)后,`POST /api/auth/login` 用账密签发 30 天会话 token(sha256 哈希存储);前端 401 弹出登录对话框(也保留手动 token 输入)
- **Token 管理界面**:设置对话框生成/吊销 API token(`api_tokens` 表,哈希存储,明文仅创建时显示一次)
- **审计日志**:创建/更新/删除/运行/登录均记录(`audit_log` 表);`GET /api/audit`;设置对话框 Audit 标签页展示
- **全局通知渠道**:`NOTIFY_TYPE`+`NOTIFY_URL`(或 config.toml)作为任务未配置通知时的回落

### 变更

- 认证中间件多源校验(静态 token / 会话 / API token);`/healthz`、`/metrics`、`/api/auth/login` 豁免认证
- 未配置任何凭据时保持完全开放(本机模式,向后兼容)

### 新增依赖

- sha2、rand(密码与 token 哈希、随机 token 生成)

## v0.6.0 「编排补全」(2026-09-16)

把成功触发链补全为完整轻编排。

### 新增

- **触发策略**:trigger_on = success(默认)/ failure / always——失败告警链、清理链成为可能
- **多下游**:trigger_task_ids 数组,一个任务结束可触发多个下游(各自独立重试/通知/链)
- **上下游传参**:下游 Shell 命令支持 {{trigger.output}} / {{trigger.status}} 插值(输出截断 10000 字符)
- **任务克隆**:卡片一键复制全部配置为新任务(name 加 (copy))
- 存量库自动迁移:旧单下游 trigger_task_id 数据并入新数组列

### 变更

- CLI --trigger-on-success 支持逗号分隔多值;新增 --trigger-on

### 修复

- 通知 send 改为基于同步 send_sync 的包装(为测试按钮提供投递反馈)

## v0.5.0 「洞察」(2026-09-16)

看得更深,调得更快:单任务调试体验全面升级。

### 新增

- **任务详情抽屉**:点击任务卡 → 元信息、总执行/成功/失败统计、平均耗时、执行历史(状态过滤、分页、内联展开输出);`GET /api/tasks/:id/stats`
- **Cron 触发预览**:表单实时显示未来 5 次触发时间(按任务时区换算),非法表达式即时红字提示;`POST /api/cron/preview`
- **通知测试按钮**:编辑任务时可向配置渠道同步发送测试通知并反馈投递结果;`POST /api/tasks/:id/notify-test`;Notifier 重构出 send_sync
- **时长趋势线**:14 天图表叠加平均耗时折线(独立毫秒轴),`/api/stats/daily` 增加每日 avg_duration_ms

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
