# 门禁系统设计

## 部署形态

i.MX6 单板运行，一个 Rust 二进制同时承担硬件控制 + REST API 服务。`tokio` 异步运行时统一调度：硬件设备用 `spawn_blocking` 轮询，通过 `mpsc` channel 向认证模块报告事件；HTTP 原生异步运行。

```
main.rs (tokio runtime)
├── spawn_blocking → NFC 轮询    ──→ mpsc → auth
├── spawn_blocking → 键盘轮询    ──→ mpsc → auth
├── spawn           → axum HTTP  ──→ web_handlers → db
└── auth 决策中心 → db / motor / buzzer
```

## 模块划分 (10 个源文件)

| # | 模块文件 | 职责 | 层级 |
|---|----------|------|------|
| 1 | `main.rs` | 入口：加载config、初始化DB、启动各子系统、信号处理优雅停机 | 编排 |
| 2 | `config.rs` | 配置结构体 + 从 `config.toml` 加载 | 基础设施 |
| 3 | `error.rs` | 统一错误类型（`AppError`），实现 `IntoResponse` | 基础设施 |
| 4 | `db.rs` | SQLite：建表/migration、用户CRUD、临时码管理、进出日志（纯同步，在 `spawn_blocking` 中调用） | 数据层 |
| 5 | `keyboard.rs` | 12键电话键盘驱动：读 `/dev/input/eventX`，keycode→字符映射，输入缓冲（`*`取消/`#`确认），通过 channel 发送完整输入码 | 驱动层 |
| 6 | `nfc.rs` | PN532 NFC驱动：UART termios 初始化(115200)→唤醒→轮询解析UID，检测到卡后通过 channel 发送 UID | 驱动层 |
| 7 | `motor.rs` | 步进电机门锁：`/dev/mem` mmap→写CPLD寄存器 `0x1C4`，`lock()`/`unlock()`，可配置自动超时回锁 | 驱动层 |
| 8 | `buzzer.rs` | 蜂鸣器：`/dev/input/event1`写`EV_SND/SND_BELL`，`success_beep()`/`failure_beep()` | 驱动层 |
| 9 | `auth.rs` | 认证核心：mpsc接收UID/临时码→调db验证→开锁/拒绝→调buzzer→记log | 业务层 |
| 10 | `web_handlers.rs` | axum handler函数：用户CRUD、临时码生成、进出日志查询、配置读写 | 接入层 |

## 数据模型 (3张表，SQLite)

```sql
CREATE TABLE users (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    nfc_uid     TEXT UNIQUE,       -- NFC卡UID hex串，NULL=仅用临时码
    phone       TEXT,
    department  TEXT,
    is_active   INTEGER DEFAULT 1,
    created_at  TEXT DEFAULT (datetime('now','localtime')),
    updated_at  TEXT DEFAULT (datetime('now','localtime'))
);

CREATE TABLE temp_codes (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id    INTEGER NOT NULL REFERENCES users(id),
    code       TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    created_at TEXT DEFAULT (datetime('now','localtime'))
);

CREATE TABLE entry_logs (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id     INTEGER REFERENCES users(id),
    auth_method TEXT NOT NULL,     -- 'nfc' | 'temp_code'
    success     INTEGER NOT NULL,
    timestamp   TEXT DEFAULT (datetime('now','localtime'))
);
```

## REST API

### 人员管理

| 方法 | 路径 | 请求体/参数 | 说明 |
|------|------|-------------|------|
| `POST` | `/api/users` | `{ name, nfc_uid?, phone?, department? }` | 注册人员 |
| `GET` | `/api/users` | `?search=` | 人员列表 |
| `GET` | `/api/users/:id` | — | 人员详情 |
| `PUT` | `/api/users/:id` | `{ name?, nfc_uid?, phone?, department?, is_active? }` | 修改信息 |
| `DELETE` | `/api/users/:id` | — | 删除（软删 is_active=0） |
| `POST` | `/api/users/:id/temp-code` | — | 生成临时识别码 |

### 记录查询

| 方法 | 路径 | 参数 | 说明 |
|------|------|------|------|
| `GET` | `/api/entry-logs` | `?date=&user_id=&page=&page_size=` | 进出记录（分页） |

### 系统配置

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/config` | 获取配置 |
| `PUT` | `/api/config` | 更新配置 |

### 其他

- 静态文件由 axum 从 `static/` 目录 serve（前端独立构建产物放到该目录即可）
- 所有 `/api/*` 返回 JSON，错误统一格式 `{ "error": "message" }`
- CORS 放开允许前端跨域开发

## 配置结构 (`config.toml`)

```toml
[devices]
nfc_uart = "/dev/ttyS2"
keyboard_input = "/dev/input/event0"
buzzer_input = "/dev/input/event1"
cpld_mem_base = "0x08000000"      # 字符串，运行时parse
cpld_mem_size = 4

[access]
lock_open_ms = 3000
temp_code_len = 6
temp_code_ttl_min = 10

[web]
listen = "0.0.0.0:8080"
static_dir = "./static"

[database]
path = "/var/lib/access_control.db"
```

## 关键事件流

### NFC刷卡进门
```
nfc轮询读卡 → 解析UID → mpsc send UID → auth接收
  → db查询users.nfc_uid → 匹配且is_active
    → ✔ motor.unlock() → buzzer.success_beep() → db写入entry_log(success=true)
    → ✘ buzzer.failure_beep() → db写入entry_log(success=false)
  → motor定时器3秒后自动 lock()
```

### 临时码进门
```
键盘按键 → 缓冲字符 → 按#确认 → mpsc send code → auth接收
  → db查询temp_codes(code, expires_at > now) → 存在即有效
    → ✔ DELETE该记录 → motor.unlock() → 成功音效 → 记log
    → ✘ 失败音效 → 记log(success=false)
```

### 生成临时码 (Web触发)
```
POST /api/users/:id/temp-code
  → rand生成n位随机数字
  → INSERT INTO temp_codes (user_id, code, expires_at)
  → 返回 { code, expires_at } 给前端展示
```
