# 门禁系统设计

## 部署形态

i.MX6 单板运行，一个 Rust 二进制同时承担硬件控制 + REST API 服务。`tokio` 异步运行时统一调度：硬件设备用 `spawn_blocking` 轮询，通过 `mpsc` channel 向认证模块报告事件；HTTP 原生异步运行。

```
main.rs (tokio runtime)
├── spawn_blocking → NFC 轮询    ──→ mpsc → auth
├── spawn_blocking → 键盘轮询    ──→ mpsc → auth
├── spawn           → axum HTTP  ──→ web_handlers → db
├── spawn           → display 管理(QT子进程, JSON Lines管道)
└── auth 决策中心 → db / motor / buzzer / display
```

## 模块划分 (11 个源文件)

| #   | 模块文件          | 职责                                                                                                             | 层级     |
| --- | ----------------- | ---------------------------------------------------------------------------------------------------------------- | -------- |
| 1   | `main.rs`         | 入口：加载config、初始化DB、启动各子系统、信号处理优雅停机                                                       | 编排     |
| 2   | `config.rs`       | 配置结构体 + 从 `config.toml` 加载                                                                               | 基础设施 |
| 3   | `error.rs`        | 统一错误类型（`AppError`），实现 `IntoResponse`                                                                  | 基础设施 |
| 4   | `db.rs`           | SQLite：建表/migration、用户CRUD、临时码管理、进出日志（纯同步，在 `spawn_blocking` 中调用）                     | 数据层   |
| 5   | `keyboard.rs`     | 12键电话键盘驱动：读 `/dev/input/eventX`，keycode→字符映射，收到字符时（`*`取消/`#`确认），通过 channel 发送字符 | 驱动层   |
| 6   | `nfc.rs`          | PN532 NFC驱动：UART termios 初始化(115200)→唤醒→轮询解析UID，检测到卡后通过 channel 发送 UID                     | 驱动层   |
| 7   | `motor.rs`        | 步进电机门锁：`/dev/mem` mmap→写CPLD寄存器 `0x1C4`，`lock()`/`unlock()`，可配置自动超时回锁                      | 驱动层   |
| 8   | `buzzer.rs`       | 蜂鸣器：`/dev/input/event1`写`EV_SND/SND_BELL`，`success_beep()`/`failure_beep()`                                | 驱动层   |
| 9   | `service.rs`         | 认证核心：mpsc接收UID/临时码→调db验证→开锁/拒绝→调buzzer+display→记log                                           | 业务层   |
| 10  | `display.rs`      | QT子进程管理：spawn QT二进制、维护stdin管道，提供 `key_press()`/`auth_result()`/`reset()` 等异步方法             | 驱动层   |
| 11  | `web.rs` | axum handler函数：用户CRUD、临时码生成、进出日志查询、配置读写                                                   | 接入层   |

### 参考文件

位于references/目录的示例代码

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

| 方法     | 路径                       | 请求体/参数                                            | 说明                     |
| -------- | -------------------------- | ------------------------------------------------------ | ------------------------ |
| `POST`   | `/api/users`               | `{ name, nfc_uid?, phone?, department? }`              | 注册人员                 |
| `GET`    | `/api/users`               | `?search=`                                             | 人员列表                 |
| `GET`    | `/api/users/:id`           | —                                                      | 人员详情                 |
| `PUT`    | `/api/users/:id`           | `{ name?, nfc_uid?, phone?, department?, is_active? }` | 修改信息                 |
| `DELETE` | `/api/users/:id`           | —                                                      | 删除（软删 is_active=0） |
| `POST`   | `/api/users/:id/temp-code` | —                                                      | 生成临时识别码           |

### 记录查询

| 方法  | 路径              | 参数                               | 说明             |
| ----- | ----------------- | ---------------------------------- | ---------------- |
| `GET` | `/api/entry-logs` | `?date=&user_id=&page=&page_size=` | 进出记录（分页） |

### 系统配置

| 方法  | 路径          | 说明     |
| ----- | ------------- | -------- |
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
path = "/path/to/your/access_control.db"

[display]
enabled = true
qt_binary = "/path/to/your/access_display"
```

## QT 显示屏子进程通信接口

### 启动方式

Rust 主进程通过 `std::process::Command` 启动 QT 二进制作为子进程，通过管道通信：

```
Rust (父进程)                     QT (子进程)
     │                                │
     ├── 写入 child.stdin ──────────► 读取 stdin (接收指令)
     │                                │
     ├── 读取 child.stdout ◄────────── 写入 stdout (反馈就绪)
```

### 协议格式

**JSON Lines** — 每条消息为一行 JSON，以 `\n` 分隔。单向通信为主，QT 仅在上报 `ready` 时回写一行。

### Rust → QT 指令 (写 stdin)

| 消息                               | 含义                          | 触发时机                   |
| ---------------------------------- | ----------------------------- | -------------------------- |
| `{"type":"init"}`                  | QT初始化，显示待机画面        | 子进程启动后立即发送       |
| `{"type":"key","char":"3"}`        | 按键输入，QT更新输入框显示    | 物理键盘每次按键           |
| `{"type":"clear"}`                 | 清空输入框                    | 按`*`键                    |
| `{"type":"submit"}`                | 已提交验证，QT显示"验证中..." | 按`#`键 或 NFC刷卡         |
| `{"type":"auth_ok","name":"张三"}` | 验证通过，QT弹窗"欢迎 张三"   | auth验证成功后             |
| `{"type":"auth_fail"}`             | 验证失败，QT弹窗"验证失败"    | auth验证失败后             |
| `{"type":"idle"}`                  | 回到待机等待状态              | 认证结果展示超时后 (如3秒) |

### QT → Rust 反馈 (写 stdout)

| 消息               | 含义                           |
| ------------------ | ------------------------------ |
| `{"type":"ready"}` | QT窗口已就绪，可以开始接收指令 |

> QT 进程启动后必须首先发送 `ready`，Rust 收到后才开始发送指令序列，避免启动时序问题。

### 典型交互序列

**临时码进门：**

```
Rust ──► QT:  {"type":"init"}
Rust ◄── QT:  {"type":"ready"}
Rust ──► QT:  {"type":"key","char":"1"}
Rust ──► QT:  {"type":"key","char":"2"}
Rust ──► QT:  {"type":"key","char":"3"}
Rust ──► QT:  {"type":"key","char":"4"}
Rust ──► QT:  {"type":"key","char":"5"}
Rust ──► QT:  {"type":"key","char":"6"}
Rust ──► QT:  {"type":"submit"}
           (auth 验证中...)
Rust ──► QT:  {"type":"auth_ok","name":"张三"}
           (延迟3秒)
Rust ──► QT:  {"type":"idle"}
```

**NFC刷卡进门：**

```
Rust ──► QT:  {"type":"submit"}
           (auth 验证中...)
Rust ──► QT:  {"type":"auth_ok","name":"李四"}
           (延迟3秒)
Rust ──► QT:  {"type":"idle"}
```

**失败场景：**

```
Rust ──► QT:  {"type":"submit"}
Rust ──► QT:  {"type":"auth_fail"}
           (延迟3秒)
Rust ──► QT:  {"type":"idle"}
```

### display.rs 模块接口

```rust
pub struct Display {
    stdin_tx: mpsc::UnboundedSender<String>,  // 写管道
}

impl Display {
    /// 启动 QT 子进程，等待 ready，返回 Display 句柄
    pub async fn new(qt_binary: &str) -> Result<Self>;

    pub fn key_press(&self, c: char);
    pub fn clear(&self);
    pub fn submit(&self);
    pub fn auth_ok(&self, name: &str);
    pub fn auth_fail(&self);
    pub fn idle(&self);
}
```

内部实现：`Display::new` 中 spawn 一个 tokio task 持有 `ChildStdin`，通过 mpsc channel 接收上层调用，逐行写入 JSON 到子进程 stdin。

## 关键事件流

### NFC刷卡进门

```
nfc轮询读卡 → 解析UID → mpsc send UID → auth接收
  → display.submit() 显示"验证中"
  → db查询users.nfc_uid → 匹配且is_active
    → ✔ motor.unlock() → buzzer.success_beep() → display.auth_ok(name)
    → ✘ buzzer.failure_beep() → display.auth_fail()
  → db写入entry_log → 延迟3秒 → motor.lock() → display.idle()
```

### 临时码进门

```
键盘按键 → 缓冲字符 → 每按一个字符 display.key_press(c)
  → 按* → display.clear()
  → 按# → display.submit() → mpsc send code → auth接收
  → db查询temp_codes(code, expires_at > now) → 存在即有效
    → ✔ DELETE该记录 → motor.unlock() → 成功音效 → display.auth_ok(name)
    → ✘ 失败音效 → display.auth_fail()
  → db写入entry_log → 延迟3秒 → motor.lock() → display.idle()
```

### 生成临时码 (Web触发)

```
POST /api/users/:id/temp-code
  → rand生成n位随机数字
  → INSERT INTO temp_codes (user_id, code, expires_at)
  → 返回 { code, expires_at } 给前端展示
```
