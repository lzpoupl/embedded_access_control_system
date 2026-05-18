# mod-db简明文档

### 1  初始化数据库

在使用数据库功能之前，请务必调用该方法：

```rust
pub fn init_db(db_path: Option<&str>) -> Result<()>;
```

如果传入参数为None，db_path会被默认为同目录下的access_control.db。

该模块采用了static变量使得不必每次调用方法先init。

```rust
static DB: OnceLock<Mutex<Connection>> = OnceLock::new();
```

该方法会在数据库中建立```users``````temp_codes``````entry_logs```三个表，采用```IF NOT EXISTS```确保不会重复建表。

### 2  插入user信息

借助模块提供的结构体```UserTableInfo```有两种方法可以插入user信息：

```rust
pub struct UserTableInfo {
    name: String,
    nfc_uid: String,
    phone: String,
    department: String,
    is_active: i32,
}
pub fn register_user(user_info: UserTableInfo) -> Result<()>;
pub fn register_users(user_infos: Vec<UserTableInfo>) -> Result<()>;
```

请注意，如果希望批量插入user信息，请务必使用```register_users```方法，该方法内采用rusqlite中的提交控制确保不会频繁对磁盘进行IO操作。

### 3  解锁判断

在该系统中解锁有两种方式：nfc刷卡，使用temp_code。

```rust
pub fn unlock_nfc(nfc_uid: &str) -> Result<bool>;
pub fn unlock_temp_code(temp_code: &str) -> Result<bool>;
```

当函数本身成功执行时，如果认证通过则返回```Ok(true)```，未通过则返回```Ok(false)```；当其未成功执行完毕时返回```Err(e)```。

### 4  申请temp_code

使用如下方法申请temp_code：

```rust
pub fn apply_temp_code(user_id: i32, valid_duration: chrono::Duration) -> Result<String>;
```

本方法传入的参数分别为user_id与临时码失效的时间长度；与上面两个方法相同，成功执行时返回```Ok(temp_code)```，为一个随机的9位数字临时码。


