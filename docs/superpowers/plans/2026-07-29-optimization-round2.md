# i-rs-schedule 优化 Round 2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 对 `crates/schedule` 做正确性/健壮性与代码质量优化:优雅停机、cron 校验、执行逻辑去重、模块重构、clippy 清零。

**Architecture:** 新建 `schedule.rs` 聚拢 `ScheduleConfig` 类型与调度计算;`Executor::execute_and_record` 统一执行记录逻辑(scheduler 与 `/run` 共用);`Scheduler` 用 `JoinSet` 跟踪在途 task 并支持 `Shutdown` 命令实现优雅停机;启动时清理僵尸 running 记录。

**Tech Stack:** Rust 2024, tokio(JoinSet/signal), rusqlite + r2d2, reqwest(Method::from_bytes), cron。

**验证约定:** AGENTS.md 声明 "No tests",本项目无测试套件。每个 task 的验证用 `cargo build && cargo clippy --all-targets && cargo fmt`,记为 VERIFY。每个 task 结尾单独 commit。本 plan 目标:`cargo clippy --all-targets` **零 warning**(清零 result_large_err)。

**API 探针已验证(本 plan 据此编写):**
- `JoinSet::spawn(future)` / `JoinSet::join_next() -> Option<Result<T, JoinError>>`
- `reqwest::Client::request(Method, url)` —— method 参数是 `reqwest::Method`,字符串需用 `Method::from_bytes(bytes).unwrap_or(Method::GET)`
- `expr.parse::<cron::Schedule>()` 返回 `Result<Schedule, cron::error::Error>`

---

## File Structure

| 文件 | 改动 | 责任 |
|------|------|------|
| `crates/schedule/src/schedule.rs` | 新建 | `ScheduleConfig` 类型 + `next_delay`/`remaining_delay`/`validate` |
| `crates/schedule/src/main.rs` | 修改 | `mod schedule;`;优雅停机协调(server 返回后发 Shutdown) |
| `crates/schedule/src/db.rs` | 修改 | 移除 `ScheduleConfig` 定义与 `now_iso` 闭包内调用;新增 `mark_stale_running_as_interrupted`;`Db::new` 调用清理 |
| `crates/schedule/src/executor.rs` | 修改 | HTTP method 用 `Method::from_bytes`;新增 `execute_and_record` |
| `crates/schedule/src/scheduler.rs` | 修改 | 删除 `impl ScheduleConfig`;用 `JoinSet`;新增 `ControlCmd::Shutdown` + 收尾 |
| `crates/schedule/src/api.rs` | 修改 | `build_task` 返回 Result + 校验;`/run` 用 `execute_and_record`;合并 err/err_msg;clippy allow |

---

## Task 1: 新建 schedule.rs,迁移 ScheduleConfig(问题 8)

**Files:**
- Create: `crates/schedule/src/schedule.rs`
- Modify: `crates/schedule/src/main.rs:4`(加 `mod schedule;`)
- Modify: `crates/schedule/src/db.rs:47-52`(删 `ScheduleConfig` 定义)
- Modify: `crates/schedule/src/scheduler.rs:22-59`(删 `impl ScheduleConfig`)

- [ ] **Step 1: 创建 schedule.rs**

创建 `crates/schedule/src/schedule.rs`,内容:

```rust
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScheduleConfig {
    Cron { expr: String },
    Once { delay_secs: u64 },
}

impl ScheduleConfig {
    /// 计算到下一次触发的时间间隔。
    ///
    /// cron 解析失败时回退到 1 小时并打 warn(防御 db 被直接写入绕过创建校验)。
    pub fn next_delay(&self) -> Duration {
        match self {
            ScheduleConfig::Cron { expr } => match expr.parse::<cron::Schedule>() {
                Ok(schedule) => {
                    let now = chrono::Utc::now();
                    match schedule.upcoming(chrono::Utc).next() {
                        Some(next) => {
                            let delta = (next - now).num_milliseconds().max(0);
                            Duration::from_millis(delta as u64)
                        }
                        None => {
                            tracing::warn!(expr = %expr, "cron yielded no upcoming fire; retrying in 1h");
                            Duration::from_secs(3600)
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, expr = %expr, "cron parse failed; retrying in 1h");
                    Duration::from_secs(3600)
                }
            },
            ScheduleConfig::Once { delay_secs } => Duration::from_secs(*delay_secs),
        }
    }

    /// 启动加载时,根据任务创建时间计算剩余延迟;已过期则返回 None。
    pub fn remaining_delay(&self, created_at: &str) -> Option<Duration> {
        match self {
            ScheduleConfig::Once { delay_secs } => match crate::db::parse_iso(created_at) {
                Some(created) => {
                    let now = chrono::Utc::now();
                    let elapsed = (now - created).num_seconds().max(0) as u64;
                    if elapsed >= *delay_secs {
                        None
                    } else {
                        Some(Duration::from_secs(*delay_secs - elapsed))
                    }
                }
                None => Some(Duration::from_secs(*delay_secs)),
            },
            ScheduleConfig::Cron { .. } => Some(self.next_delay()),
        }
    }

    /// 校验调度配置是否合法。cron 表达式无效时返回错误信息。
    pub fn validate(&self) -> Result<(), String> {
        match self {
            ScheduleConfig::Cron { expr } => match expr.parse::<cron::Schedule>() {
                Ok(_) => Ok(()),
                Err(e) => Err(format!("invalid cron expr {expr:?}: {e}")),
            },
            ScheduleConfig::Once { .. } => Ok(()),
        }
    }
}
```

- [ ] **Step 2: main.rs 注册模块**

修改 `crates/schedule/src/main.rs` 顶部模块声明(line 1-4):

```rust
mod api;
mod db;
mod executor;
mod schedule;
mod scheduler;
```

- [ ] **Step 3: db.rs 移除 ScheduleConfig 定义,re-export**

修改 `crates/schedule/src/db.rs`:删除 `ScheduleConfig` 枚举定义(line 47-52),并在文件顶部 `use` 之后加 re-export,使现有 `use crate::db::ScheduleConfig` 调用点(api.rs/scheduler.rs)无需改动:

删除:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScheduleConfig {
    Cron { expr: String },
    Once { delay_secs: u64 },
}
```

在 `use std::path::Path;` 之后(`now_iso` 定义之前)插入:
```rust
pub use crate::schedule::ScheduleConfig;
```

- [ ] **Step 4: scheduler.rs 删除 impl ScheduleConfig 块**

修改 `crates/schedule/src/scheduler.rs`:删除 line 22-59 整个 `impl ScheduleConfig { ... }` 块(`next_delay` + `remaining_delay` 两个方法,它们已移到 schedule.rs)。保留文件顶部的 `use crate::db::{Db, ScheduleConfig, Task, TaskExecution};` 不变(通过 db 的 re-export 仍可用)。

注意:`remaining_delay` 内调 `crate::db::parse_iso` —— 在 schedule.rs 里也是这么写的,一致。

- [ ] **Step 5: VERIFY**

```bash
cargo build 2>&1 | tail -3 && cargo clippy --all-targets 2>&1 | grep -E 'warning|error' | head && cargo fmt && cargo fmt --check && echo "fmt clean"
```
Expected: 构建通过;clippy 仅剩预存的 `result_large_err`(Task 7 清零);fmt clean。

- [ ] **Step 6: Commit**

```bash
git add crates/schedule/src/schedule.rs crates/schedule/src/main.rs crates/schedule/src/db.rs crates/schedule/src/scheduler.rs
git commit -m "refactor: extract ScheduleConfig into schedule.rs module

Move ScheduleConfig type and its next_delay/remaining_delay methods out of
db.rs/scheduler.rs into a dedicated schedule.rs. db.rs re-exports the type
so existing call sites are unchanged. next_delay now warns on cron parse
failure instead of silently falling back to 1h."
```

---

## Task 2: cron 创建时强校验(问题 4)

**Files:**
- Modify: `crates/schedule/src/api.rs:59-86`(`build_task` 返回 Result)
- Modify: `crates/schedule/src/api.rs:104-114`(POST handler)
- Modify: `crates/schedule/src/api.rs:147-165`(PUT handler)

- [ ] **Step 1: build_task 返回 Result 并校验**

修改 `crates/schedule/src/api.rs` 的 `build_task`(line 59-86),签名改为返回 `Result<Task, String>`,在 `Task::new` 前加校验:

```rust
fn build_task(body: CreateTaskRequest) -> Result<Task, String> {
    let task_type = match body.task_type.as_deref() {
        Some("shell") => TaskType::Shell {
            cmd: body.shell_cmd.unwrap_or_default(),
        },
        _ => TaskType::Http {
            method: body.http_method.unwrap_or_else(|| "GET".into()),
            url: body.http_url.unwrap_or_default(),
            headers: body.http_headers,
            body: body.http_body,
        },
    };

    let schedule = match body.schedule_type.as_deref() {
        Some("once") => ScheduleConfig::Once {
            delay_secs: body.delay_secs.unwrap_or(0),
        },
        _ => ScheduleConfig::Cron {
            expr: body.cron_expr.unwrap_or_default(),
        },
    };

    schedule.validate()?;

    let mut task = Task::new(body.name, task_type, schedule);
    if let Some(enabled) = body.enabled {
        task.enabled = enabled;
    }
    Ok(task)
}
```

- [ ] **Step 2: POST handler 处理 Result**

修改 `crates/schedule/src/api.rs` POST `/api/tasks` handler(line 108 附近):

替换:
```rust
            let task = build_task(body);
```
为:
```rust
            let task = build_task(body).map_err(|e| err_msg(400, e))?;
```

- [ ] **Step 3: PUT handler 处理 Result**

修改 `crates/schedule/src/api.rs` PUT `/api/tasks/:id` handler(line 156 附近):

替换:
```rust
            let mut task = build_task(body);
```
为:
```rust
            let mut task = build_task(body).map_err(|e| err_msg(400, e))?;
```

- [ ] **Step 4: VERIFY**

```bash
cargo build 2>&1 | tail -3 && cargo clippy --all-targets 2>&1 | grep -E 'warning|error' | head && cargo fmt && cargo fmt --check && echo "fmt clean"
```
Expected: 构建通过;clippy 仅剩预存 warning。

- [ ] **Step 5: Commit**

```bash
git add crates/schedule/src/api.rs
git commit -m "feat(api): validate cron expression on task create/update

build_task returns Result and calls ScheduleConfig::validate; POST and PUT
return 400 with the parse error on invalid cron instead of silently storing
a broken schedule."
```

---

## Task 3: now_iso 移出闭包 + HTTP method 任意化(问题 5, 10)

**Files:**
- Modify: `crates/schedule/src/db.rs`(`update_task`/`set_enabled`/`update_execution`)
- Modify: `crates/schedule/src/executor.rs:38-51`(`execute_http` method 匹配)

- [ ] **Step 1: db.rs 三处 now_iso 移出闭包**

`update_task`(line 322-373):当前 `let now = now_iso();` 在闭包内(line 326)。移到闭包外。修改后该方法的 `spawn_db` 调用段:

```rust
    pub async fn update_task(&self, task: &Task) -> anyhow::Result<()> {
        let pool = self.pool.clone();
        let task = task.clone();
        let now = now_iso();
        spawn_db(pool, move |conn| {
            let (schedule_type, cron_expr, delay_secs) = match &task.schedule {
                ScheduleConfig::Cron { expr } => ("cron", Some(expr.as_str()), None),
                ScheduleConfig::Once { delay_secs } => ("once", None, Some(*delay_secs as i64)),
            };
            let (task_type_str, method, url, headers, body, cmd) = match &task.task_type {
                TaskType::Http {
                    method,
                    url,
                    headers,
                    body,
                } => {
                    let h = headers.as_ref().and_then(|v| serde_json::to_string(v).ok());
                    (
                        "http",
                        Some(method.as_str()),
                        Some(url.as_str()),
                        h,
                        body.as_deref(),
                        None,
                    )
                }
                TaskType::Shell { cmd } => ("shell", None, None, None, None, Some(cmd.as_str())),
            };
            conn.execute(
                "UPDATE tasks SET name=?1, task_type=?2, enabled=?3, schedule_type=?4, cron_expr=?5,
                 delay_secs=?6, http_method=?7, http_url=?8, http_headers=?9, http_body=?10,
                 shell_cmd=?11, updated_at=?12 WHERE id=?13",
                params![
                    task.name,
                    task_type_str,
                    task.enabled as i64,
                    schedule_type,
                    cron_expr,
                    delay_secs,
                    method.unwrap_or("GET"),
                    url.unwrap_or(""),
                    headers,
                    body,
                    cmd.unwrap_or(""),
                    now,
                    task.id,
                ],
            )?;
            Ok(())
        })
        .await
    }
```

`set_enabled`(line 389-401):同样移出:

```rust
    pub async fn set_enabled(&self, id: &str, enabled: bool) -> anyhow::Result<bool> {
        let pool = self.pool.clone();
        let id = id.to_string();
        let now = now_iso();
        spawn_db(pool, move |conn| {
            let affected = conn.execute(
                "UPDATE tasks SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
                params![enabled as i64, now, &id],
            )?;
            Ok(affected > 0)
        })
        .await
    }
```

`update_execution`(line 425-445):同样移出:

```rust
    pub async fn update_execution(
        &self,
        exec_id: &str,
        status: &str,
        output: &str,
        http_status: Option<i64>,
    ) -> anyhow::Result<()> {
        let pool = self.pool.clone();
        let exec_id = exec_id.to_string();
        let status = status.to_string();
        let output = output.to_string();
        let now = now_iso();
        spawn_db(pool, move |conn| {
            conn.execute(
                "UPDATE task_executions SET status=?1, output=?2, http_status=?3, finished_at=?4 WHERE id=?5",
                params![status, output, http_status, now, exec_id],
            )?;
            Ok(())
        })
        .await
    }
```

- [ ] **Step 2: executor.rs HTTP method 任意化**

修改 `crates/schedule/src/executor.rs` 的 `execute_http`(line 45-51),替换 match 为 `Method::from_bytes`:

替换:
```rust
        let mut req = match method.to_uppercase().as_str() {
            "GET" => self.client.get(url),
            "POST" => self.client.post(url),
            "PUT" => self.client.put(url),
            "DELETE" => self.client.delete(url),
            _ => self.client.get(url),
        };
```
为:
```rust
        let method = reqwest::Method::from_bytes(method.to_uppercase().as_bytes())
            .unwrap_or(reqwest::Method::GET);
        let mut req = self.client.request(method, url);
```

- [ ] **Step 3: VERIFY**

```bash
cargo build 2>&1 | tail -3 && cargo clippy --all-targets 2>&1 | grep -E 'warning|error' | head && cargo fmt && cargo fmt --check && echo "fmt clean"
```
Expected: 构建通过;clippy 仅剩预存 warning。

- [ ] **Step 4: Commit**

```bash
git add crates/schedule/src/db.rs crates/schedule/src/executor.rs
git commit -m "refactor: hoist now_iso out of blocking closures; support any HTTP method

- update_task/set_enabled/update_execution now capture now_iso() in the
  caller rather than inside the spawn_blocking closure.
- execute_http uses Method::from_bytes + Client::request so PATCH/HEAD/...
  are passed through instead of silently falling back to GET."
```

---

## Task 4: 提取 Executor::execute_and_record + /run 复用(问题 9, 2, 3)

**Files:**
- Modify: `crates/schedule/src/executor.rs`(新增 `execute_and_record`)
- Modify: `crates/schedule/src/scheduler.rs:134-191`(spawn 块改调用)
- Modify: `crates/schedule/src/api.rs:232-272`(`/run` handler 改调用)

- [ ] **Step 1: executor.rs 新增 execute_and_record**

修改 `crates/schedule/src/executor.rs`,在 `impl Executor` 内(`execute` 方法之后)新增方法。需要 import `Db` 与 `TaskExecution`:

文件顶部 use 改为:
```rust
use crate::db::{Db, Task, TaskExecution, TaskType};
use std::time::Duration;
```
(原来只有 `use crate::db::{Task, TaskType};`)

在 `pub async fn execute(...)` 方法之后插入:

```rust
    /// 执行任务并记录到 DB。封装 create_execution → execute → update_execution,
    /// scheduler 与 `/run` 端点共用,保证行为一致。
    ///
    /// create_execution 失败时返回一个内存构造的 `status="skipped"` 记录(不入库),
    /// 并打 warn 日志;execute 与 update_execution 的失败均告警但不影响返回。
    pub async fn execute_and_record(&self, db: &Db, task: &Task) -> TaskExecution {
        let exec_id = uuid::Uuid::new_v4().to_string();
        let started_at = crate::db::now_iso();
        let start = std::time::Instant::now();

        let running = TaskExecution {
            id: exec_id.clone(),
            task_id: task.id.clone(),
            status: "running".to_string(),
            output: None,
            http_status: None,
            started_at: started_at.clone(),
            finished_at: None,
        };

        if let Err(e) = db.create_execution(&running).await {
            tracing::warn!(
                error = %e,
                task_id = %task.id,
                "create execution failed; skipping run"
            );
            return TaskExecution {
                status: "skipped".to_string(),
                finished_at: Some(crate::db::now_iso()),
                ..running
            };
        }

        tracing::info!(task_id = %task.id, exec_id = %exec_id, "execution started");

        let result = self.execute(task).await;

        if let Err(e) = db
            .update_execution(&exec_id, &result.status, &result.output, result.http_status)
            .await
        {
            tracing::warn!(
                error = %e,
                task_id = %task.id,
                exec_id = %exec_id,
                "update execution failed"
            );
        }

        tracing::info!(
            task_id = %task.id,
            exec_id = %exec_id,
            status = %result.status,
            duration_ms = start.elapsed().as_millis() as u64,
            "execution finished"
        );

        db.get_execution(&exec_id)
            .await
            .ok()
            .flatten()
            .unwrap_or(running)
    }
```

> **说明:** 末尾 `db.get_execution` 失败时回退到内存 `running` 记录(状态仍是 running,但此时实际已执行完——这是 best-effort,正常路径 get_execution 不会失败)。比原来的 `.unwrap()` 安全。

- [ ] **Step 2: scheduler spawn 块改调用 execute_and_record**

修改 `crates/schedule/src/scheduler.rs`,替换 run 循环内 `tokio::spawn(async move { ... })` 块(line 134-191)。当前 spawn 块包含 create_execution/execute/update_execution 全套逻辑,改为调用 `execute_and_record`。

替换:
```rust
                    tokio::spawn(async move {
                        let exec_id = Uuid::new_v4().to_string();
                        let started_at = crate::db::now_iso();
                        let start = std::time::Instant::now();

                        if let Err(e) = db
                            .create_execution(&TaskExecution {
                                id: exec_id.clone(),
                                task_id: task_clone.id.clone(),
                                status: "running".to_string(),
                                output: None,
                                http_status: None,
                                started_at,
                                finished_at: None,
                            })
                            .await
                        {
                            tracing::warn!(
                                error = %e,
                                task_id = %task_clone.id,
                                "create execution failed; skipping run"
                            );
                            return;
                        }

                        tracing::info!(
                            task_id = %task_clone.id,
                            exec_id = %exec_id,
                            "execution started"
                        );

                        let result = exec.execute(&task_clone).await;

                        if let Err(e) = db
                            .update_execution(
                                &exec_id,
                                &result.status,
                                &result.output,
                                result.http_status,
                            )
                            .await
                        {
                            tracing::warn!(
                                error = %e,
                                task_id = %task_clone.id,
                                exec_id = %exec_id,
                                "update execution failed"
                            );
                        }

                        tracing::info!(
                            task_id = %task_clone.id,
                            exec_id = %exec_id,
                            status = %result.status,
                            duration_ms = start.elapsed().as_millis() as u64,
                            "execution finished"
                        );
                    });
```
为:
```rust
                    tokio::spawn(async move {
                        let _ = exec.execute_and_record(&db, &task_clone).await;
                    });
```

注意:此 task 暂用裸 `tokio::spawn`(Task 5 会把它换成 `JoinSet`)。`uuid::Uuid`/`TaskExecution` 等 import 在 scheduler.rs 若因此变 unused,Task 5 会一并清理;此步 VERIFY 若报 unused import 警告,把 `use uuid::Uuid;` 删掉(它原本只服务 spawn 块的 exec_id 生成)。

- [ ] **Step 3: /run handler 改调用 execute_and_record**

修改 `crates/schedule/src/api.rs` 的 `/run` handler(line 232-272)。替换整个 handler 体内、`task` 取出之后到 `ok(exec_record)` 的部分:

替换:
```rust
            let exec_id = uuid::Uuid::new_v4().to_string();
            let started_at = chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string();
            let _ = db
                .create_execution(&TaskExecution {
                    id: exec_id.clone(),
                    task_id: task.id.clone(),
                    status: "running".to_string(),
                    output: None,
                    http_status: None,
                    started_at: started_at.clone(),
                    finished_at: None,
                })
                .await;

            let result = exec.execute(&task).await;
            let _ = db
                .update_execution(&exec_id, &result.status, &result.output, result.http_status)
                .await;

            let exec_record = db
                .get_execution(&exec_id)
                .await
                .map_err(|e| err_msg(500, format!("db error: {e}")))?
                .unwrap();
            ok(exec_record)
```
为:
```rust
            let exec_record = exec.execute_and_record(&db, &task).await;
            ok(exec_record)
```

这消除了 `/run` 的 `let _ =`(静默吞错)与 `.unwrap()`(panic 风险)。

- [ ] **Step 4: VERIFY**

```bash
cargo build 2>&1 | tail -3 && cargo clippy --all-targets 2>&1 | grep -E 'warning|error' | head && cargo fmt && cargo fmt --check && echo "fmt clean"
```
Expected: 构建通过。可能 `scheduler.rs` 的 `use uuid::Uuid;` 变 unused → 删除它。可能 `api.rs` 的 `use crate::db::TaskExecution` 变 unused → 删除(但 `api.rs` 用 `TaskExecution` 只在此处,确认后删)。clippy 仅剩预存 warning。

- [ ] **Step 5: Commit**

```bash
git add crates/schedule/src/executor.rs crates/schedule/src/scheduler.rs crates/schedule/src/api.rs
git commit -m "refactor: extract execute_and_record; dedupe /run and scheduler

Executor::execute_and_record encapsulates create_execution → execute →
update_execution with the structured logs. The scheduler spawn block and
the /run endpoint both call it, so behavior is uniform. /run no longer
silently swallows db errors nor unwrap()s the final get_execution."
```

---

## Task 5: JoinSet + ControlCmd::Shutdown + 优雅停机(问题 1)

**Files:**
- Modify: `crates/schedule/src/scheduler.rs`(JoinSet + Shutdown + 收尾)
- Modify: `crates/schedule/src/main.rs`(server 返回后发 Shutdown,等 scheduler 收尾)

- [ ] **Step 1: ControlCmd 新增 Shutdown**

修改 `crates/schedule/src/scheduler.rs`,`ControlCmd` 枚举(line 11-15)加变体:

```rust
pub enum ControlCmd {
    Add(Task),
    Remove(String),
    Update(Task),
    Shutdown,
}
```

- [ ] **Step 2: Scheduler 持有 JoinSet**

修改 `crates/schedule/src/scheduler.rs` 的 `Scheduler` 结构(line 17-20)与 `new`:

```rust
pub struct Scheduler {
    queue: DelayQueue<Task>,
    keys: HashMap<String, delay_queue::Key>,
    join_set: tokio::task::JoinSet<()>,
}
```

`new`:
```rust
    pub fn new() -> Self {
        Self {
            queue: DelayQueue::new(),
            keys: HashMap::new(),
            join_set: tokio::task::JoinSet::new(),
        }
    }
```

- [ ] **Step 3: run 循环改用 join_set.spawn + 处理 Shutdown**

修改 `crates/schedule/src/scheduler.rs` 的 `run` 方法。把 spawn 处的 `tokio::spawn` 改为 `self.join_set.spawn`;在 `cmd_rx.recv()` 分支处理 `Shutdown` 后退出循环。

替换 run 方法末尾的控制命令 match 分支(line 194-209 附近的 `Some(cmd) = cmd_rx.recv() =>` 分支):

```rust
                Some(cmd) = cmd_rx.recv() => {
                    match cmd {
                        ControlCmd::Add(task) => {
                            tracing::debug!(task_id = %task.id, "scheduler add");
                            self.insert(task);
                        }
                        ControlCmd::Remove(id) => {
                            tracing::debug!(task_id = %id, "scheduler remove");
                            self.remove(&id);
                        }
                        ControlCmd::Update(task) => {
                            tracing::debug!(task_id = %task.id, "scheduler update");
                            self.update(task);
                        }
                        ControlCmd::Shutdown => {
                            tracing::info!("scheduler shutdown requested; draining in-flight executions");
                            break;
                        }
                    }
                }
```

并把到期任务处的 `tokio::spawn(async move { ... })` 改为 `self.join_set.spawn(async move { ... })`。

- [ ] **Step 4: run 末尾等待在途 task 收尾**

在 `crates/schedule/src/scheduler.rs` 的 `run` 方法中,`loop { ... }` 之后(break 跳出后)加收尾逻辑:

```rust
        // 收到 Shutdown 后,等待在途 execution 完成(HTTP/shell 各自有超时,
        // 这里再加一个总 timeout 兜底,避免某个卡死的 task 拖住退出)。
        let drain = async {
            while self.join_set.join_next().await.is_some() {}
        };
        if let Err(_) = tokio::time::timeout(Duration::from_secs(35), drain).await {
            tracing::warn!("shutdown drain timed out after 35s; aborting remaining tasks");
            self.join_set.abort_all();
        }
        tracing::info!("scheduler stopped");
```

注意:`run` 方法签名 `pub async fn run(mut self, ...)` —— `self` 被 move,`self.join_set` 在 break 后仍可用。Duration 已在文件顶部 import(`use std::time::Duration;` 已有)。

- [ ] **Step 5: main.rs 协调 server 与 scheduler 停机**

修改 `crates/schedule/src/main.rs`。当前(line 33-42):
```rust
    tokio::spawn(async move {
        scheduler.run(cmd_rx, executor, scheduler_db).await;
    });

    let router = api::build_router(db, cmd_tx, api_executor);

    tracing::info!("starting server on http://{addr}");
    desirable::new(&addr).run(router).await?;

    Ok(())
```

改为(server 收到 SIGINT 返回后,发 Shutdown 让 scheduler 收尾):

```rust
    let router = api::build_router(db, cmd_tx.clone(), api_executor);

    tracing::info!("starting server on http://{addr}");

    // server.run 内部已处理 SIGINT:收到信号后停止 accept 并返回 Ok(())。
    // 之后我们通知 scheduler 进入优雅停机,等待在途 execution 收尾。
    let server = desirable::new(&addr);
    let _ = server.run(router).await;
    tracing::info!("server stopped; signaling scheduler to shut down");
    let _ = cmd_tx.send(scheduler::ControlCmd::Shutdown);
    scheduler.run(cmd_rx, executor, scheduler_db).await;

    Ok(())
```

> **关键点:** scheduler 不再提前 `tokio::spawn`。server.run 返回(SIGNAL 后)才调 scheduler.run——此时 scheduler 立即收到 Shutdown(因为 cmd_tx 已发),走 drain 分支。在 server 运行期间,scheduler 没在跑——这是错的!
>
> **修正:** 必须让 scheduler 在 server 运行期间就在跑,server 返回后再触发它收尾。用 `tokio::select!` 或先 spawn 再 await。正确写法:

```rust
    let router = api::build_router(db, cmd_tx.clone(), api_executor);

    tracing::info!("starting server on http://{addr}");

    // scheduler 先在后台跑,处理到期任务与控制命令。
    let scheduler_handle = tokio::spawn(async move {
        scheduler.run(cmd_rx, executor, scheduler_db).await;
    });

    let server = desirable::new(&addr);
    let _ = server.run(router).await;
    tracing::info!("server stopped; signaling scheduler to shut down");
    let _ = cmd_tx.send(scheduler::ControlCmd::Shutdown);

    // 等 scheduler 完成 drain(在途 execution 收尾)。
    let _ = scheduler_handle.await;

    Ok(())
```

**以此修正版为准。** `cmd_tx` 在 build_router 里被 clone 进 router 闭包,所以这里用 `cmd_tx.clone()`;build_router 之后原 `cmd_tx` 仍可用于发 Shutdown。但注意:当前 `build_router(db, cmd_tx, ...)` 是 move 语义,会消费 cmd_tx。需在调用前 clone。完整 main.rs 见 Step 5 最终版。

**main.rs 最终版(完整):**

```rust
mod api;
mod db;
mod executor;
mod schedule;
mod scheduler;

use db::Db;
use executor::Executor;
use scheduler::{ControlCmd, Scheduler};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let db_path = std::env::var("SCHEDULE_DB").unwrap_or_else(|_| "./data/schedule.db".to_string());
    let port = std::env::var("SCHEDULE_PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("127.0.0.1:{port}");

    let db = Db::new(&db_path)?;

    let tasks = db.list_enabled_tasks().await?;
    tracing::info!("loaded {} enabled tasks from db", tasks.len());

    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();

    let mut scheduler = Scheduler::new();
    scheduler.load_tasks(tasks);

    let executor = Arc::new(Executor::new());
    let scheduler_db = db.clone();
    let api_executor = executor.clone();

    // scheduler 在后台跑,处理到期任务与控制命令。
    let scheduler_handle = tokio::spawn(async move {
        scheduler.run(cmd_rx, executor, scheduler_db).await;
    });

    let router = api::build_router(db, cmd_tx.clone(), api_executor);

    tracing::info!("starting server on http://{addr}");

    // server.run 内部已处理 SIGINT:收到信号后停止 accept 并返回 Ok(())。
    let server = desirable::new(&addr);
    let _ = server.run(router).await;
    tracing::info!("server stopped; signaling scheduler to shut down");
    let _ = cmd_tx.send(ControlCmd::Shutdown);

    // 等 scheduler 完成 drain(在途 execution 收尾)。
    let _ = scheduler_handle.await;

    Ok(())
}
```

- [ ] **Step 6: VERIFY**

```bash
cargo build 2>&1 | tail -3 && cargo clippy --all-targets 2>&1 | grep -E 'warning|error' | head && cargo fmt && cargo fmt --check && echo "fmt clean"
```
Expected: 构建通过;clippy 仅剩预存 warning。注意 `scheduler.rs` 顶部 `use uuid::Uuid;` 可能已 unused(Step 4 的 spawn 块不再直接用 Uuid),确认删除。

- [ ] **Step 7: Commit**

```bash
git add crates/schedule/src/scheduler.rs crates/schedule/src/main.rs
git commit -m "feat(scheduler): graceful shutdown via JoinSet and ControlCmd::Shutdown

- Scheduler tracks spawned executions in a JoinSet instead of bare tokio::spawn
- ControlCmd::Shutdown breaks the run loop
- On shutdown, drain in-flight executions with a 35s timeout, then abort
- main.rs spawns the scheduler, runs the server, and on SIGINT sends
  Shutdown and awaits the scheduler's drain before exiting"
```

---

## Task 6: 启动时清理僵尸 running 记录(问题 1)

**Files:**
- Modify: `crates/schedule/src/db.rs`(新增 `mark_stale_running_as_interrupted`;`Db::new` 调用)

- [ ] **Step 1: 新增 mark_stale_running_as_interrupted 方法**

在 `crates/schedule/src/db.rs` 的 `impl Db` 内(`init_schema` 之后或任意位置)加:

```rust
    /// 将上次进程残留的 `status='running'` 执行记录标记为 `interrupted`。
    /// 进程崩溃/被杀后,这些记录会永远停在 running;启动时调用本方法修复可观察性。
    pub async fn mark_stale_running_as_interrupted(&self) -> anyhow::Result<u64> {
        let pool = self.pool.clone();
        let now = now_iso();
        spawn_db(pool, move |conn| {
            let affected = conn.execute(
                "UPDATE task_executions SET status = ?1, finished_at = ?2 WHERE status = 'running'",
                params!["interrupted", now],
            )?;
            Ok(affected as u64)
        })
        .await
    }
```

- [ ] **Step 2: Db::new 调用清理**

修改 `crates/schedule/src/db.rs` 的 `Db::new`(line 172-186)。`init_schema` 之后加调用。但 `mark_stale_running_as_interrupted` 是 async,而 `new` 是同步 fn。两个选择:
- 把 `new` 改 async(影响 main.rs 调用点,改 `.await`)
- 在 `new` 里用同步方式直接执行(不走 spawn_db)

**选后者**(最小侵入,`new` 保持同步)。在 `new` 里直接用 `pool.get()` 同步执行 UPDATE:

修改 `Db::new`:
```rust
    pub fn new(db_path: &str) -> anyhow::Result<Self> {
        if let Some(parent) = Path::new(db_path).parent() {
            std::fs::create_dir_all(parent).context("create db parent dir")?;
        }
        let manager = r2d2_sqlite::SqliteConnectionManager::file(db_path);
        let pool = r2d2::Pool::builder()
            .max_size(8)
            .min_idle(Some(1))
            .connection_customizer(Box::new(SqliteInit))
            .build(manager)
            .context("build sqlite pool")?;
        let db = Self { pool };
        db.init_schema()?;
        db.cleanup_stale_running()?;
        Ok(db)
    }

    fn cleanup_stale_running(&self) -> anyhow::Result<()> {
        let conn = self.pool.get()?;
        let now = now_iso();
        let affected = conn.execute(
            "UPDATE task_executions SET status = ?1, finished_at = ?2 WHERE status = 'running'",
            params!["interrupted", now],
        )?;
        if affected > 0 {
            tracing::info!(
                affected,
                "marked stale running executions as interrupted on startup"
            );
        }
        Ok(())
    }
```

> **决策:** 用私有同步方法 `cleanup_stale_running`,不暴露 async 版本(YAGNI——只有启动时调用一次)。删除 Step 1 的 async `mark_stale_running_as_interrupted`,只保留 `cleanup_stale_running`。**以 Step 2 的写法为准**,Step 1 跳过。

- [ ] **Step 3: VERIFY**

```bash
cargo build 2>&1 | tail -3 && cargo clippy --all-targets 2>&1 | grep -E 'warning|error' | head && cargo fmt && cargo fmt --check && echo "fmt clean"
```
Expected: 构建通过;clippy 仅剩预存 warning。

- [ ] **Step 4: Commit**

```bash
git add crates/schedule/src/db.rs
git commit -m "feat(db): mark stale running executions as interrupted on startup

If the process was killed mid-execution, task_executions rows stay in
'running' forever. Db::new now resets them to 'interrupted' at startup
and logs how many were affected. Adds the 'interrupted' status value
(no schema change — status is TEXT)."
```

---

## Task 7: 合并 err/err_msg + 清零 clippy(问题 6, 7)

**Files:**
- Modify: `crates/schedule/src/api.rs:16-36`(ok + err + err_msg)

- [ ] **Step 1: 合并 err 到 err_msg,给 ok 加 allow**

修改 `crates/schedule/src/api.rs:16-36`。替换三个 helper:

```rust
#[allow(clippy::result_large_err)]
fn ok<T: Serialize>(data: T) -> Result<Response, Response> {
    let body = ApiResponse {
        code: 0,
        message: "ok".into(),
        data: serde_json::to_value(&data).unwrap_or(serde_json::Value::Null),
    };
    Ok(Response::json(body))
}

fn err_msg(status: u16, msg: impl Into<String>) -> Response {
    let body = ApiResponse {
        code: status,
        message: msg.into(),
        data: serde_json::Value::Null,
    };
    Response::with_status(status, serde_json::to_string(&body).unwrap()).unwrap()
}
```

> `err()` 被删除(它原本只被 `err_msg` 调用)。所有调用点用的是 `err_msg`,无需改动。`ok` 上的 `#[allow(clippy::result_large_err)]` 配合注释(可选)说明 Response 大小由 desirable 框架决定。

如希望更明确,在 `ok` 上方加注释:
```rust
// Response 的 Err 变体较大(由 desirable 框架定义,无法瘦身);
// boxing 会改变返回类型引发连锁,故 allow 掉 result_large_err。
```

- [ ] **Step 2: VERIFY(应清零)**

```bash
cargo build 2>&1 | tail -3 && cargo clippy --all-targets 2>&1 | grep -E 'warning|error' | head && cargo fmt && cargo fmt --check && echo "fmt clean"
```
Expected: 构建通过;**clippy 零 warning**(result_large_err 已 allow)。这是本 plan 的关键验收点。

- [ ] **Step 3: Commit**

```bash
git add crates/schedule/src/api.rs
git commit -m "refactor(api): merge err into err_msg; allow result_large_err

err() was only ever called by err_msg(), so they collapse into one. The
result_large_err clippy warning on ok() is now explicitly allowed with a
comment: Response's size is dictated by the desirable framework and cannot
be shrunk without changing the return type. clippy is now warning-free."
```

---

## Task 8: 全量验证 + 手动冒烟

**Files:** 无改动

- [ ] **Step 1: 全量构建 + clippy 清零确认**

```bash
cargo build --all-targets 2>&1 | tail -3
echo "=== clippy(应为空)==="
cargo clippy --all-targets 2>&1 | grep -E '^warning|^error'
echo "=== fmt ==="
cargo fmt --check && echo "fmt clean"
```
Expected: 构建成功;clippy grep 无输出(零 warning);fmt clean。

- [ ] **Step 2: 手动冒烟 — 基础功能**

终端 1(后台启动,用临时 db/port):
```bash
SCHEDULE_DB=/tmp/r2-smoke.db SCHEDULE_PORT=3607 /Users/mankong/volumes/code/i-rs/i-rs-schedule/target/debug/i-rs-schedule > /tmp/r2-server.log 2>&1 &
echo "pid: $!"
sleep 2
```
预期日志: `loaded 0 enabled tasks` + `starting server` + `Listening`

终端 2:
```bash
# 1. 创建合法 cron 任务
curl -s -X POST http://localhost:3607/api/tasks \
  -H 'Content-Type: application/json' \
  -d '{"name":"ok","task_type":"shell","schedule_type":"cron","cron_expr":"0 * * * * *","shell_cmd":"echo hi"}' | python3 -m json.tool
# 预期: code=0, 有 id

# 2. 创建非法 cron → 应 400
curl -s -X POST http://localhost:3607/api/tasks \
  -H 'Content-Type: application/json' \
  -d '{"name":"bad","task_type":"shell","schedule_type":"cron","cron_expr":"not-a-cron","shell_cmd":"echo hi"}' | python3 -m json.tool
# 预期: code=400, message 含 "invalid cron expr"

# 3. /run 触发 HTTP 任务(PATCH 验证 method 任意化)
curl -s -X POST http://localhost:3607/api/tasks \
  -H 'Content-Type: application/json' \
  -d '{"name":"http-patch","task_type":"http","schedule_type":"once","delay_secs":3600,"http_method":"PATCH","http_url":"https://httpbin.org/patch"}' > /tmp/r2-tasks.json
TID=$(python3 -c "import json;print(json.load(open('/tmp/r2-tasks.json'))['data']['id'])")
curl -s -X POST http://localhost:3607/api/tasks/$TID/run | python3 -m json.tool
# 预期: status=success, http_status=200(PATCH 被正确发送,而非 fallback 成 GET)
```

- [ ] **Step 3: 手动冒烟 — 优雅停机 + 僵尸清理**

```bash
# 1. 创建一个长 shell 任务(用 sleep 模拟在途),立即发 SIGINT
curl -s -X POST http://localhost:3607/api/tasks \
  -H 'Content-Type: application/json' \
  -d '{"name":"long","task_type":"shell","schedule_type":"once","delay_secs":2,"shell_cmd":"sleep 20; echo done"}' > /dev/null
sleep 3   # 等任务到期开始执行(sleep 20 在跑)
# 此时发 SIGINT
pkill -INT -f 'target/debug/i-rs-schedule'
sleep 2
echo "=== server 日志(应有 drain + stopped)==="
grep -E 'draining|stopped|finished|execution' /tmp/r2-server.log | tail -15
```
预期: 看到 `scheduler shutdown requested; draining in-flight executions`,随后 `execution finished`(等待 sleep 20 完成,可能触发 35s timeout → `timed out; aborting`),最后 `scheduler stopped`。

```bash
# 2. 验证僵尸清理:用 kill -9 模拟崩溃,留下 running 记录
SCHEDULE_DB=/tmp/r2-smoke.db SCHEDULE_PORT=3607 /Users/mankong/volumes/code/i-rs/i-rs-schedule/target/debug/i-rs-schedule > /tmp/r2-server2.log 2>&1 &
PID=$!
sleep 2
# 触发一个长任务,然后 kill -9
curl -s -X POST http://localhost:3607/api/tasks \
  -H 'Content-Type: application/json' \
  -d '{"name":"kill9","task_type":"shell","schedule_type":"once","delay_secs":1,"shell_cmd":"sleep 30"}' > /dev/null
sleep 2
kill -9 $PID
# 重启
SCHEDULE_DB=/tmp/r2-smoke.db SCHEDULE_PORT=3607 /Users/mankong/volumes/code/i-rs/i-rs-schedule/target/debug/i-rs-schedule > /tmp/r2-server3.log 2>&1 &
sleep 2
echo "=== 启动日志(应有 marked stale running)==="
grep -E 'marked stale|interrupted' /tmp/r2-server3.log
# 验证 db 里那条记录状态
sqlite3 /tmp/r2-smoke.db "SELECT name, status FROM task_executions te JOIN tasks t ON te.task_id=t.id WHERE t.name='kill9';"
# 预期: status=interrupted
```

- [ ] **Step 4: 清理 + 终结 commit**

```bash
pkill -f 'target/debug/i-rs-schedule' 2>/dev/null
rm -f /tmp/r2-smoke.db* /tmp/r2-*.log /tmp/r2-tasks.json
git status
# 若干净则跳过;若有 fmt 残留:
# git add -A && git commit -m "style: final rustfmt pass"
```

---

## 完成后产物

- **schedule.rs**:ScheduleConfig 类型与全部调度计算/校验集中一处
- **cron 创建校验**:POST/PUT 对非法 cron 返回 400,不再静默存储坏 schedule
- **execute_and_record**:scheduler 与 /run 共用执行记录逻辑,行为统一,/run 不再吞错/panic
- **优雅停机**:SIGINT 后 scheduler drain 在途 execution(35s timeout 兜底),无僵尸
- **僵尸清理**:启动时把残留 running 标为 interrupted
- **clippy 零 warning**
- 共 7 个语义化 commit
