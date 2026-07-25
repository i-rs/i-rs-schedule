# Design: Tokio-SQLite 定时任务系统

## 1. 整体架构

Workspace 结构:

```
i-rs-schedule/
├── Cargo.toml          # workspace root
└── crates/
    ├── schedule/       # 调度服务
    └── cli/            # CLI 工具
```

数据流:

```
main → init_db → load_tasks → start_scheduler → start_web_server
                                │
        ┌───────────────────────┼────────────────────────┐
        ▼                       ▼                        ▼
   API (desirable)        Scheduler              Executor
                          (DelayQueue)           (http/shell)
        │                       │                        │
        └───────────┬───────────┘                        │
                    ▼                                    │
              DB (SQLite) ◄──────────────────────────────┘
                     (写执行记录)
```

- `schedule` crate: desirable web 服务 + DelayQueue 调度器 + 执行器
- `cli` crate: clap CLI，纯 HTTP 客户端调用 schedule 的 REST API，不依赖 schedule crate

---

## 2. SQLite Schema

```sql
CREATE TABLE tasks (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    task_type   TEXT NOT NULL DEFAULT 'http',    -- 'http' | 'shell'
    enabled     INTEGER NOT NULL DEFAULT 1,
    schedule_type TEXT NOT NULL DEFAULT 'cron',  -- 'cron' | 'once'
    cron_expr   TEXT,
    delay_secs  INTEGER,
    http_method TEXT DEFAULT 'GET',
    http_url    TEXT,
    http_headers TEXT,
    http_body   TEXT,
    shell_cmd   TEXT,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE task_executions (
    id          TEXT PRIMARY KEY,
    task_id     TEXT NOT NULL,
    status      TEXT NOT NULL,       -- 'running' | 'success' | 'failure'
    output      TEXT,
    http_status INTEGER,
    started_at  TEXT NOT NULL DEFAULT (datetime('now')),
    finished_at TEXT,
    FOREIGN KEY (task_id) REFERENCES tasks(id)
);
```

---

## 3. Scheduler 核心（统一 DelayQueue）

所有任务统一插入 `tokio_util::time::DelayQueue`，key 为到期时间，value 为 Task。

主循环:

```
select {
    task = delay_queue.next() =>
        tokio::spawn(execute(task))
        if task.schedule == Cron => recalc next_fire_at, re-insert

    cmd = control_rx.recv() =>
        Add(task)    => delay_queue.insert(task, next_fire_at)
        Remove(id)   => delay_queue.remove(&key_map[id])
        Update(task) => remove + re-insert
}
```

- 维护 `HashMap<TaskId, DelayQueue::Key>` 做反查
- API handler 通过 `mpsc::Sender<ControlCmd>` 通知 scheduler
- 启动加载: 从 DB 取所有 enabled=true 的任务逐个 insert
- 并发: 每个任务独立 `tokio::spawn`，不做限制

---

## 4. REST API

| Method | Path                    | Description      |
|--------|-------------------------|------------------|
| POST   | /api/tasks              | 创建任务         |
| GET    | /api/tasks              | 列出所有任务     |
| GET    | /api/tasks/:id          | 查看单个任务     |
| PUT    | /api/tasks/:id          | 更新任务         |
| DELETE | /api/tasks/:id          | 删除任务         |
| POST   | /api/tasks/:id/enable   | 启用             |
| POST   | /api/tasks/:id/disable  | 禁用             |
| GET    | /api/executions         | 查询执行记录     |
| GET    | /api/executions/:id     | 单条执行详情     |

---

## 5. CLI 命令

```
i-rs-cli task add    --name --type http|shell [--cron expr | --delay secs] [...]
i-rs-cli task list   [--enabled]
i-rs-cli task show   --id
i-rs-cli task rm     --id
i-rs-cli task enable  --id
i-rs-cli task disable --id
i-rs-cli exec list   [--task-id] [--limit]
```

`--server` 参数或 `SCHEDULE_SERVER` 环境变量指定 schedule 地址，默认 `http://localhost:3000`。

---

## 6. 依赖

**schedule crate**: desirable, tokio, tokio-util, rusqlite (bundled), reqwest, cron, serde/serde_json, uuid, chrono, tracing, anyhow

**cli crate**: clap, reqwest, serde/serde_json, uuid, anyhow

CLI 不依赖 schedule crate，完全通过 HTTP API 通信。

---

## 7. 配置

| 环境变量         | 默认值               | 说明            |
|-----------------|---------------------|-----------------|
| SCHEDULE_PORT   | 3000                | 服务端口        |
| SCHEDULE_DB     | ./data/schedule.db  | SQLite 路径     |
| RUST_LOG        | info                | 日志级别        |

---

## 8. 模块划分

`crates/schedule/src/`:

- `main.rs` — 启动流程
- `db.rs` — rusqlite: init, task CRUD, execution CRUD
- `scheduler.rs` — DelayQueue 循环, 控制命令处理
- `executor.rs` — HTTP 回调执行, Shell 命令执行
- `api.rs` — desirable 路由定义

`crates/cli/src/`:

- `main.rs` — clap 命令解析, HTTP 请求, 结果展示
