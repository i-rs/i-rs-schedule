# Design: i-rs-schedule 代码优化

对现有 Rust workspace(`crates/schedule` + `crates/cli`)的性能、可维护性、健壮性优化。不改 API 路由签名、不改 schema、不动 frontend。

## 现状问题清单(已确认)

| # | 类别 | 问题 |
|---|------|------|
| 1 | 性能 | `Db` 用 `Arc<Mutex<Connection>>`,所有 DB 操作全局串行 |
| 2 | 性能 | `Executor::execute_http` 每次新建 `reqwest::Client`,丢失连接池/TLS 复用 |
| 3 | 健壮性 | HTTP 请求无超时,长尾任务可挂死 spawn 出的执行线程 |
| 4 | 健壮性 | scheduler 中 execution 写库用 `let _ =` 静默吞错 |
| 5 | 健壮性 | 关键路径(到期/开始/完成/失败)无日志 |
| 6 | 健壮性 | `.await??` 与 `.await?` 语义不一致 |
| 7 | 正确性 | `build_update_task` 硬编码 `enabled: true`,PUT 无法禁用任务 |
| 8 | 可维护 | 时间格式串 `%Y-%m-%dT%H:%M:%S%.3fZ` 散落 6 处 |
| 9 | 可维护 | `remaining_delay` 两段重复的 `parse_from_str` fallback |
| 10 | 可维护 | `row_to_task`/`row_to_execution` 包在怪异的 `private` 模块里 |
| 11 | 可维护 | `build_task` 与 `build_update_task` 近乎完全重复(约 60 行) |
| 12 | 可维护 | CLI 每个子命令重复 `send().await? → json → println` 样板 |

---

## 1. DB 层:引入 r2d2 连接池

### 决策
- 用 `r2d2 = "0.8"` + `r2d2_sqlite = "0.35"`(后者直接兼容当前 `rusqlite = 0.40`,无需降级)
- **不**用 deadpool-sqlite(0.13.0 锁 rusqlite ^0.38,不兼容)
- `rusqlite` 保持 0.40,`bundled` feature 不变

### 实现
- `Db` 内部: `Arc<Mutex<Connection>>` → `r2d2::Pool<SqliteConnectionManager>`
- 每个方法仍走 `spawn_blocking`(`rusqlite` 同步,`PooledConnection` 获取与查询都阻塞)
- 每条连接通过 `r2d2::CustomizeConnection` 在建连时执行 `PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;`
- 池配置:`max_size = 8`,`min_idle = Some(1)`
- `Db` 仍 `Clone`(pool 内部 `Arc`),API 与 scheduler 共享同一池
- 提取 helper `spawn_db<F, T>(pool, f) -> Result<T>`,内部 `pool.get()? → f(conn) → spawn_blocking` 一条龙,消除每处重复的 `.await??`

### 方法签名
全部保持不变(`create_task(&self, task: &Task) -> Result<()>` 等)。调用方(api.rs、scheduler.rs)零改动。

### `private` 模块处理
`row_to_task` / `row_to_execution` 移出 `mod private`,作为 `db.rs` 顶层自由函数(可保留 `pub(crate)` 或私有)。`mod private` 删除。

---

## 2. Executor:复用 HTTP Client + 超时

### Client 复用
- `Executor` 从单元结构体改为持有 `reqwest::Client`
- `Executor::new()` 构造一个 `Client`,所有 HTTP 执行复用
- `main.rs`:`Arc::new(Executor)` → `Arc::new(Executor::new())`

### 超时
- `Client::builder().timeout(Duration::from_secs(30)).build()`
- 覆盖 connect + request 全程,长尾任务超时返回 failure 而非无限挂起
- 超时错误信息与其它网络错误一致走 `ExecutionResult { status: "failure", output: e.to_string() }`

### 不改动
- `execute_http` / `execute_shell` 的签名与返回结构
- HTTP method 匹配、headers/body 处理逻辑

---

## 3. 消除重复 + 常量化

### 时间格式
```rust
const TIMESTAMP_FMT: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";
fn now_iso() -> String {
    chrono::Utc::now().format(TIMESTAMP_FMT).to_string()
}
```
散落 6 处(db.rs 3 处、api.rs 1 处、scheduler.rs 2 处)统一引用。`now_iso()` 放 `db.rs` 或独立的 `util.rs`(倾向放 `db.rs` 顶层,因为时间戳主要服务 DB 记录)。

### ISO 解析 helper
```rust
fn parse_iso(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::NaiveDateTime::parse_from_str(s, TIMESTAMP_FMT)
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.fZ"))
        .ok()
        .map(|dt| dt.and_utc())
}
```
`remaining_delay` 中两段重复 fallback 合并为一次调用。

### build_task 合并
- 删除 `build_update_task`
- `build_task` 返回 `Task`(用 `Task::new` 生成 id + 时间戳)
- 更新场景:api.rs 的 PUT handler 改为 `Task::new(...)` 后**保留原 id**(从 path 取),用 `Task { id: original_id, ..build_task(body) }` 覆盖 id
- 因 `enabled` 需可更新(见第 5 节),`build_task` 产出的 `enabled` 用请求体或缺省 `true`

### CLI 样板
提取 `print_response<T: DeserializeOwned>(resp: reqwest::Response) -> Result<()>`:
```rust
async fn print_response(resp: reqwest::Response) -> Result<()> {
    let val: serde_json::Value = resp.json().await?;
    println!("{}", serde_json::to_string_pretty(&val)?);
    Ok(())
}
```
每个子命令的 `let result: Value = resp.json().await?; println!(...)` 缩为一行 `print_response(resp).await?`。

---

## 4. 错误处理与日志

### execution 写库不再静默
scheduler spawn 内:
```rust
if let Err(e) = db.create_execution(&exec).await {
    tracing::warn!(error = %e, task_id = %task.id, "create execution failed");
    return; // 没有执行记录,无法追踪,放弃本次执行
}
let result = exec.execute(&task).await;
if let Err(e) = db.update_execution(&exec_id, &result.status, &result.output, result.http_status).await {
    tracing::warn!(error = %e, %exec_id, "update execution failed");
}
```
（语义:create 失败则跳过执行——没有记录的执行比晚一点执行更糟;update 失败仅告警,执行本身已完成。)

### 结构化日志
| 位置 | 级别 | 字段 |
|------|------|------|
| 任务到期(DelayQueue 弹出) | `debug!` | task_id, name |
| 执行开始 | `info!` | task_id, exec_id |
| 执行完成 | `info!` | task_id, exec_id, status, duration_ms |
| HTTP 执行 reqwest 返回 Err | `warn!` | task_id, url, error |
| scheduler 控制命令 | `debug!` | cmd variant, task_id |
| `load_tasks` 丢弃过期 Once | `info!` | task_id, name |

### `.await??` 统一
全部走第 1 节的 `spawn_db` helper,内部统一处理 `JoinError` + 业务错误。db.rs 中不再出现裸 `.await??` / `.await?` 混用。

---

## 5. 边界语义修正

### 5.1 PUT 支持 enabled
- `CreateTaskRequest` 增 `enabled: Option<bool>`
- `build_task`:缺省仍 `true`(POST 创建启用),有传则用传入值
- PUT handler:`Task { id: path_id, ..build_task(body) }`,`enabled` 由 body 控制(不传则保持启用语义)
- `db.update_task` 已写 enabled 字段,无需改 SQL

### 5.2 Once 任务语义
维持现状(过期丢弃)。`load_tasks` 中 `remaining_delay` 返回 `None` 时加 `info!` 日志(当前静默)。

### 5.3 不改动
- desirable Router 用法、路由表、响应格式
- frontend
- schema(无迁移)
- CLI 子命令结构

---

## 6. 依赖变更

`crates/schedule/Cargo.toml` 新增:
```toml
r2d2 = "0.8"
r2d2_sqlite = "0.35"
```
`rusqlite = { version = "0.40.1", features = ["bundled"] }` 不变。
`crates/cli/Cargo.toml` 不变。

## 7. 验证

- `cargo build` 通过
- `cargo clippy` 无新增 warning
- `cargo fmt` 已格式化
- 手动冒烟(可选):启动服务,CLI 增删查任务、触发 run、查看 execution

## 8. 实施顺序(每步独立可验证 commit)

1. 引入 r2d2,重构 `Db` 为 pool,移除 `private` 模块,加 `spawn_db` helper
2. `Executor` 复用 Client + 超时
3. 提取 `TIMESTAMP_FMT` / `now_iso` / `parse_iso`,消除时间格式重复
4. 合并 `build_task` / `build_update_task`
5. PUT 支持 `enabled` 字段
6. CLI `print_response` helper 去样板
7. scheduler/db 错误处理 + 结构化日志
8. `load_tasks` 过期 Once 日志

每步后 `cargo build && cargo clippy && cargo fmt`。
