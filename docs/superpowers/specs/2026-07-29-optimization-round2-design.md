# Design: i-rs-schedule 代码优化 Round 2

对现有 Rust workspace(`crates/schedule`)做正确性/健壮性与代码质量优化。不改 API 路由表、不改 schema(仅新增 status 字符串值)、不动 frontend、不改 CLI 子命令结构。

## 问题清单(已确认)

### 正确性/健壮性
| # | 问题 | 位置 |
|---|------|------|
| 1 | Scheduler 无优雅停机;SIGINT 后在途 execution 被直接切断,留下僵尸 running 记录 | main.rs / scheduler.rs |
| 2 | `/run` 端点 `let _ =` 静默吞掉 create/update execution 错误,与 scheduler 分支行为不一致 | api.rs:248,261 |
| 3 | `/run` 端点 `db.get_execution(&exec_id).unwrap()` 在 create 失败时 panic | api.rs:269 |
| 4 | cron 表达式创建时不校验;运行时解析失败静默回退 1 小时 | scheduler.rs:33,36 / api.rs build_task |
| 5 | `now_iso()` 在 spawn_blocking 闭包内调用,时间应在调用方取 | db.rs update_task/set_enabled/update_execution |
| 10 | HTTP method 匹配把未知 method 静默当 GET | executor.rs:50 |

### 代码质量
| # | 问题 |
|---|------|
| 6 | `err()` 仅被 `err_msg()` 调用一次,冗余双 helper |
| 7 | 预存 clippy warning `result_large_err`(`ok<T>() -> Result<Response, Response>` Err 变体 144 字节) |
| 8 | `ScheduleConfig` 的 `impl`(`next_delay`/`remaining_delay`)写在 scheduler.rs,类型却在 db.rs,职责错位 |
| 9 | `/run` handler 与 scheduler 的执行逻辑(create→execute→update)重复,且 `/run` 版本更糙 |

---

## 1. 优雅停机(问题 1)

### 约束
`desirable::Server::run` 已内置 SIGINT 处理:收到信号后停止 accept 并返回 `Ok(())`(见 server.rs `run_graceful`)。但 main 的 scheduler spawn 无人收尾,在途 execution task 随进程退出被切断。

### Scheduler 改造
- 内部用 `tokio::task::JoinSet<Task>` 跟踪所有 spawn 的 execution task,替代裸 `tokio::spawn`
- `ControlCmd` 新增变体 `Shutdown`,run 主循环收到后退出 select loop
- `run` 末尾:`join_set.abort()` 不合适(会丢正在跑的);改为 `join_set.join_all().await`(等待所有在途完成)+ 整体 timeout 守护。考虑到 HTTP/shell 任务已有各自超时(HTTP 30s),用 `tokio::time::timeout(Duration::from_secs(35), join_set.join_all())` 兜底

### main.rs 改造
- 不再 `tokio::spawn(scheduler.run(...))`,改为:
  ```
  tokio::select! {
      res = scheduler.run(cmd_rx.clone(), executor.clone(), db.clone()) => res,
      _ = server.run(router) => {
          // server 收到 SIGINT 返回,通知 scheduler 停机
          let _ = cmd_tx.send(ControlCmd::Shutdown);
          scheduler.run(...).await  // 收尾
      }
  }
  ```
  具体实现见 plan Task 1;核心是 server 返回后向 scheduler 发 Shutdown,让 scheduler 走收尾分支。

### 僵尸清理
- `Db` 新增方法 `mark_stale_running_as_interrupted() -> Result<u64>`:`UPDATE task_executions SET status='interrupted', finished_at=? WHERE status='running'`,返回受影响行数
- `Db::new` 中 `init_schema` 之后调用,记 `info!` 日志(行数)
- `interrupted` 是新 status 字符串值,无需 schema 迁移(status 列本就是 TEXT)
- 含义:进程上次崩溃/被杀,残留的 running 记录标记为 interrupted,可观察

### 不改动
- desirable 框架(已正确处理 server 侧停机)
- HTTP/shell 执行器各自的超时(executor.rs 已有 30s)

---

## 2. 健壮性修正(问题 2-5, 10)

### 问题 2+3: `/run` 端点统一执行逻辑
- `/run` handler 改为调用 `Executor::execute_and_record(&db, &task)`(见第 3 节问题 9),返回的 `TaskExecution` 直接 `ok()` 给客户端
- 消除 `let _ =`(静默吞错)与 `.unwrap()`(panic 风险),行为与 scheduler 分支一致

### 问题 4: cron 创建时强校验
- `ScheduleConfig` 新增 `validate(&self) -> Result<(), String>`(放 schedule.rs),`Cron { expr }` 时尝试 `expr.parse::<cron::Schedule>()`,失败返回 `Err(format!("invalid cron expr {expr:?}: {e}"))`;`Once` 恒 Ok
- `build_task` 签名改为 `fn build_task(body) -> Result<Task, String>`,内部 `task.schedule.validate()?`
- POST/PUT handler:`build_task(body).map_err(|e| err_msg(400, e))?`
- 运行时兜底:`next_delay` 解析失败处加 `tracing::warn!(task_id, expr, "cron parse failed; retrying in 1h")`,保留 1h 回退(防御 db 被直接写入绕过校验的情况)

### 问题 5: now_iso 移出闭包
`update_task`、`set_enabled`、`update_execution` 三处:`let now = now_iso();` 从闭包内移到闭包外(随参数 move 进闭包)。语义正确(时间在调用方取),且减少阻塞线程工作量。

### 问题 10: HTTP method 支持任意值
`executor.rs:45-51` 的 match 改为:
```rust
let method = method.to_uppercase();
let mut req = self.client.request(method.as_str(), url);
```
用 `reqwest::Client::request(method, url)`,支持任意 HTTP method(GET/POST/PUT/DELETE/PATCH/HEAD/...);畸形 method 由 reqwest 返回错误,走现有 `Err` 分支报 failure。删除 match。

---

## 3. 代码质量重构(问题 6-9)

### 问题 8: 独立 schedule.rs 模块
- 新建 `crates/schedule/src/schedule.rs`:
  - `ScheduleConfig` 枚举定义(从 db.rs 移入)
  - `impl ScheduleConfig { next_delay, remaining_delay, validate }`(从 scheduler.rs 移入 + 新增 validate)
- `db.rs`:删除 `ScheduleConfig` 定义;顶部 `use crate::schedule::ScheduleConfig;`(因 `Task` 字段引用它,`row_to_task`/`create_task` 等需用)
- `scheduler.rs`:删除 `impl ScheduleConfig` 块,`use crate::schedule::ScheduleConfig;`(模式匹配用)
- `api.rs`:已是 `use crate::db::{..., ScheduleConfig, ...}`,改为从 `crate::schedule` 引入或保留 db 的 re-export。**决策**:`db.rs` 顶部 `pub use crate::schedule::ScheduleConfig;` 做 re-export,这样 api.rs/cli 等现有 `use crate::db::ScheduleConfig` 无需改动,降低耦合面
- `main.rs`:加 `mod schedule;`

### 问题 9: 提取 Executor::execute_and_record
- `Executor` 新增方法:
  ```rust
  pub async fn execute_and_record(&self, db: &Db, task: &Task) -> TaskExecution
  ```
- 内部逻辑(与当前 scheduler spawn 块一致):
  1. 生成 exec_id、started_at、start Instant
  2. `db.create_execution(running)`;失败 → `warn!` + 返回一个 status=interrupted 的占位 TaskExecution(见下文语义讨论)
  3. `info!` execution started
  4. `self.execute(task).await`
  5. `db.update_execution(final)`;失败 → `warn!`(不掩盖完成)
  6. `info!` execution finished(duration_ms)
  7. 重新 `db.get_execution(exec_id)` 返回完整记录
- **create_execution 失败的返回语义**:scheduler 侧当前是 `return`(不执行,无记录)。统一后,`execute_and_record` 在 create 失败时也 return,但需返回某个 TaskExecution 给调用方。**决策**:create_execution 失败时返回 `TaskExecution { status: "skipped", ... }`(内存构造,不入库),让 `/run` 能给客户端明确反馈;同时 warn 日志记录。scheduler spawn 侧忽略返回值即可
- scheduler spawn 块与 `/run` handler 都改为调 `execute_and_record`

### 问题 7: 修复 clippy result_large_err
- `ok<T>()` 函数上加 `#[allow(clippy::result_large_err)]` 并附注释:Response 大小由 desirable 框架决定,无法瘦身,boxing 会改变返回类型引发连锁
- 彻底清零 clippy warning

### 问题 6: 合并 err/err_msg
- 删除 `err()`;`err_msg` 内联 `err` 的实现:
  ```rust
  fn err_msg(status: u16, msg: impl Into<String>) -> Response {
      let body = ApiResponse { code: status, message: msg.into(), data: serde_json::Value::Null };
      Response::with_status(status, serde_json::to_string(&body).unwrap()).unwrap()
  }
  ```
- 所有调用点(本就是 `err_msg`)无需改动

### 不改动
- schema DDL(仅 status 新增 interrupted/skipped 字符串值)
- desirable Router 用法、路由表
- frontend
- CLI

---

## 4. 依赖与模块结构

### 依赖
无新增。`cron`、`chrono`、`tokio`(signal feature 已在 "full")、`reqwest` 均已在。

### 模块结构(改动后)
```
crates/schedule/src/
├── main.rs       — 启动 + 优雅停机协调
├── schedule.rs   — ScheduleConfig 类型 + next_delay/remaining_delay/validate (新)
├── db.rs         — Task/TaskType/TaskExecution 类型 + Db(pool) + row mapper
├── executor.rs   — Executor(Client) + execute + execute_and_record
├── scheduler.rs  — Scheduler(DelayQueue) + run 循环 + Shutdown 命令
└── api.rs        — Router + build_task(带校验) + err_msg
```

---

## 5. 验证

- `cargo build` 通过
- `cargo clippy --all-targets` **零 warning**(清零 result_large_err)
- `cargo fmt --check` 通过
- 手动冒烟:
  - POST 创建任务(含非法 cron → 400)
  - PUT 更新(含非法 cron → 400)
  - `/run` 触发 HTTP 任务 → 返回完整 execution
  - SIGINT 后观察:在途 task 完成、无残留 running 记录(下次启动 mark_stale 应返回 0)
  - 制造僵尸(直接 kill -9 后重启)→ 启动日志显示标记行数

## 6. 实施顺序(每步独立可验证 commit)

1. 新建 schedule.rs,迁移 ScheduleConfig 类型与 impl(问题 8)
2. 加 cron validate;build_task 返回 Result + handler 校验(问题 4)
3. now_iso 移出闭包(问题 5)+ HTTP method 任意化(问题 10)
4. 提取 Executor::execute_and_record;/run 与 scheduler 改用它(问题 9 + 2 + 3)
5. JoinSet + ControlCmd::Shutdown + main 优雅停机协调(问题 1)
6. Db::mark_stale_running_as_interrupted + 启动清理(问题 1)
7. 合并 err/err_msg(问题 6)+ clippy allow(问题 7)
8. 全量验证 + 手动冒烟

每步后 `cargo build && cargo clippy --all-targets && cargo fmt`。
