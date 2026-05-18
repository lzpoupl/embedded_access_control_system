use rusqlite::{Connection, Result, params};
use std::sync::{Mutex, OnceLock};

static DB: OnceLock<Mutex<Connection>> = OnceLock::new();

pub struct UserTableInfo {
    name: String,
    nfc_uid: String,
    phone: String,
    department: String,
    is_active: i32,
}

pub fn init_db(db_path: &str) -> Result<()> {
    // 如果是debug模式，直接使用内存数据库做简单测试
    let db_conn = if cfg!(debug_assertions) {
        Connection::open_in_memory()?
    } else {
        Connection::open(db_path)?
    };
    db_conn.execute("PRAGMA foreign_keys = ON", [])?;
    // 创建users表
    db_conn.execute("
        CREATE TABLE IF NOT EXISTS users (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            name        TEXT NOT NULL,
            nfc_uid     TEXT UNIQUE,       -- NFC卡UID hex串, NULL=仅用临时码
            phone       TEXT,
            department  TEXT,
            is_active   INTEGER DEFAULT 1,
            created_at  TEXT DEFAULT (datetime('now','localtime')),
            updated_at  TEXT DEFAULT (datetime('now','localtime'))
        )", 
[],
    )?;
    // 为users表设置触发器，更新行内容时候自动更新
    db_conn.execute("
        CREATE TRIGGER trg_users_updated_at
            AFTER UPDATE ON users
            FOR EACH ROW
        BEGIN
            UPDATE users SET updated_at = datetime('now','localtime') WHERE id = OLD.id;
        END", 
    [],
    )?;
    // 创建temp_codes表
    db_conn.execute("
        CREATE TABLE IF NOT EXISTS temp_codes (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id    INTEGER NOT NULL REFERENCES users(id),
            code       TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            created_at TEXT DEFAULT (datetime('now','localtime'))
        )", 
        [],
    )?;
    // 创建entry_logs表
    db_conn.execute("
        CREATE TABLE IF NOT EXISTS entry_logs (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id     INTEGER,
            auth_method TEXT NOT NULL,     -- 'nfc' | 'temp_code'
            success     INTEGER NOT NULL,
            timestamp   TEXT DEFAULT (datetime('now','localtime'))
        )", 
        [],
    )?;
    // 设置全局变量DB
    DB.set(Mutex::new(db_conn))
        .map_err(|_| rusqlite::Error::InvalidParameterName("数据库已初始化".into()))?;
    Ok(())
}

fn get_db() -> std::sync::MutexGuard<'static, Connection> {
    DB.get()
        .expect("数据库未初始化，请先调用 init_db()")
        .lock()
        .expect("数据库锁被污染")
}

pub fn register_user(user_info: UserTableInfo) -> Result<()> {
    let db_conn = get_db();
    db_conn.execute("
        INSERT INTO users (name, nfc_uid, phone, department, is_active)
        VALUES (?1, ?2, ?3, ?4, ?5)", 
        params![
            &user_info.name,
            &user_info.nfc_uid,
            &user_info.phone,
            &user_info.department,
            &user_info.is_active,
        ]
    )?;
    Ok(())
}
pub fn register_users(user_infos: Vec<UserTableInfo>) -> Result<()> {
    let mut db_conn = get_db();
    let tx = db_conn.transaction()?;
    let mut stmt = tx.prepare("
        INSERT INTO users (name, nfc_uid, phone, department, is_active)
        VALUES (?1, ?2, ?3, ?4, ?5)
    ")?;
    for user_info in &user_infos {
        stmt.execute((
            &user_info.name,
            &user_info.nfc_uid,
            &user_info.phone,
            &user_info.department,
            &user_info.is_active,
        ))?;
    }
    drop(stmt);
    tx.commit()?;
    Ok(())
}
// 刷卡解锁
pub fn unlock_nfc(nfc_uid: &str) -> Result<bool> {
    let db_conn = get_db();
    let result = db_conn.query_row("
        SELECT id FROM users WHERE nfc_uid = ?1 LIMIT 1", 
    [nfc_uid], 
        |row| row.get::<_, i32>(0),
    );
    let (user_id, success, return_value) =  match result {
        Ok(id) => (Some(id), 1, Ok(true)),
        Err(rusqlite::Error::QueryReturnedNoRows) => (None, 0, Ok(false)),
        Err(e) => {
            return Err(e);
        }
    };

    db_conn.execute("
        INSERT INTO entry_logs (user_id, auth_method, success)
        VALUES (?1, 'nfc', ?2)", 
        (user_id, success),
    )?;
    return_value
}