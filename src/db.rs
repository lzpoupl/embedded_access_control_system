use rusqlite::{Connection, Result, params};
use chrono::{Duration, Local};

pub struct UserTableInfo {
    name: String,
    nfc_uid: String,
    phone: String,
    department: String,
    is_active: i32,
}
impl UserTableInfo {
    pub fn new(name: String, nfc_uid: String, phone: String, department: String, is_active: i32) -> Self {
        Self {
            name,
            nfc_uid,
            phone,
            department,
            is_active,
        }
    }
}

pub fn init_db(db_path: Option<&str>) -> Result<Connection> {
    // 如果是debug模式，直接使用内存数据库做简单测试
    let db_conn = 
        Connection::open(db_path.unwrap_or("./access_control.db"))?;
    
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
        CREATE TRIGGER IF NOT EXISTS trg_users_updated_at
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
    Ok(db_conn)
}


pub fn bind_new_nfc(db_conn: &Connection, user_id: i32, new_nfc_uid: &str) -> Result<()> {
    db_conn.execute("
        UPDATE users SET nfc_uid = ?1 WHERE id = ?2", 
        params![new_nfc_uid, user_id],
    )?;
    Ok(())
}
pub fn register_user(db_conn: &Connection, user_info: UserTableInfo) -> Result<()> {
    db_conn.execute("
        INSERT OR IGNORE INTO users (name, nfc_uid, phone, department, is_active)
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
pub fn register_users(db_conn: &mut Connection, user_infos: Vec<UserTableInfo>) -> Result<()> {
    let tx = db_conn.transaction()?;
    let mut stmt = tx.prepare("
        INSERT OR IGNORE INTO users (name, nfc_uid, phone, department, is_active)
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
pub fn unlock_nfc(db_conn: &Connection, nfc_uid: &str) -> Result<bool> {
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
pub fn unlock_temp_code(db_conn: &Connection, temp_code: &str) -> Result<bool> {
    let result = db_conn.query_row("
        SELECT user_id FROM temp_codes WHERE code = ?1 AND expires_at > datetime('now','localtime') LIMIT 1", 
    [temp_code], 
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
        VALUES (?1, 'temp_code', ?2)", 
        (user_id, success),
    )?;
    return_value
}

// 申请temp_code
pub fn apply_temp_code(db_conn: &Connection, user_id: i32, valid_duration: Duration) -> Result<String> {
    let now = Local::now();
    let expires_at = now + valid_duration;
    let temp_code = format!("{:9}", rand::random::<u32>() % 1_000_000_000); // 生成9位随机码
    db_conn.execute("
        INSERT INTO temp_codes (user_id, code, expires_at)
        VALUES (?1, ?2, ?3)", 
        params![
            user_id,
            &temp_code,
            expires_at.format("%Y-%m-%d %H:%M:%S").to_string(),
        ]
    )?;
    Ok(temp_code)
}

pub struct UserRow {
    pub id: i32,
    pub name: String,
    pub nfc_uid: Option<String>,
    pub phone: Option<String>,
    pub department: Option<String>,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

pub struct UserUpdate {
    pub name: Option<String>,
    pub nfc_uid: Option<String>,
    pub phone: Option<String>,
    pub department: Option<String>,
    pub is_active: Option<bool>,
}

pub struct EntryLogRow {
    pub id: i32,
    pub user_id: Option<i32>,
    pub auth_method: String,
    pub success: bool,
    pub timestamp: String,
}

fn map_row_to_user(row: &rusqlite::Row) -> rusqlite::Result<UserRow> {
    Ok(UserRow {
        id: row.get(0)?,
        name: row.get(1)?,
        nfc_uid: row.get(2)?,
        phone: row.get(3)?,
        department: row.get(4)?,
        is_active: row.get::<_, i32>(5)? != 0,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

pub fn list_users(db_conn: &Connection, search: Option<&str>) -> Result<Vec<UserRow>> {
    if let Some(keyword) = search {
        let pattern = format!("%{}%", keyword);
        let mut stmt = db_conn.prepare(
            "SELECT id, name, nfc_uid, phone, department, is_active, created_at, updated_at \
             FROM users WHERE name LIKE ?1 OR nfc_uid LIKE ?1 OR phone LIKE ?1 OR department LIKE ?1 \
             ORDER BY id DESC"
        )?;
        let rows = stmt.query_map(params![pattern], map_row_to_user)?
            .collect::<rusqlite::Result<Vec<UserRow>>>()?;
        Ok(rows)
    } else {
        let mut stmt = db_conn.prepare(
            "SELECT id, name, nfc_uid, phone, department, is_active, created_at, updated_at \
             FROM users ORDER BY id DESC"
        )?;
        let rows = stmt.query_map([], map_row_to_user)?
            .collect::<rusqlite::Result<Vec<UserRow>>>()?;
        Ok(rows)
    }
}

pub fn get_user_by_id(db_conn: &Connection, id: i32) -> Result<Option<UserRow>> {
    let mut stmt = db_conn.prepare(
        "SELECT id, name, nfc_uid, phone, department, is_active, created_at, updated_at \
         FROM users WHERE id = ?1"
    )?;
    let mut rows = stmt.query_map(params![id], map_row_to_user)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

pub fn update_user(db_conn: &Connection, id: i32, updates: &UserUpdate) -> Result<bool> {
    let mut set_clauses: Vec<String> = Vec::new();
    let mut values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(ref name) = updates.name {
        set_clauses.push(format!("name = ?{}", values.len() + 1));
        values.push(Box::new(name.clone()));
    }
    if let Some(ref nfc_uid) = updates.nfc_uid {
        set_clauses.push(format!("nfc_uid = ?{}", values.len() + 1));
        values.push(Box::new(nfc_uid.clone()));
    }
    if let Some(ref phone) = updates.phone {
        set_clauses.push(format!("phone = ?{}", values.len() + 1));
        values.push(Box::new(phone.clone()));
    }
    if let Some(ref department) = updates.department {
        set_clauses.push(format!("department = ?{}", values.len() + 1));
        values.push(Box::new(department.clone()));
    }
    if let Some(is_active) = updates.is_active {
        set_clauses.push(format!("is_active = ?{}", values.len() + 1));
        values.push(Box::new(is_active as i32));
    }

    if set_clauses.is_empty() {
        return Ok(false);
    }

    values.push(Box::new(id));
    let sql = format!(
        "UPDATE users SET {} WHERE id = ?{}",
        set_clauses.join(", "),
        values.len()
    );

    let params_refs: Vec<&dyn rusqlite::types::ToSql> = values.iter().map(|v| v.as_ref()).collect();
    let affected = db_conn.execute(&sql, params_refs.as_slice())?;
    Ok(affected > 0)
}

pub fn delete_user(db_conn: &Connection, id: i32) -> Result<bool> {
    let affected = db_conn.execute(
        "UPDATE users SET is_active = 0 WHERE id = ?1 AND is_active = 1",
        params![id],
    )?;
    Ok(affected > 0)
}

pub fn list_entry_logs(
    db_conn: &Connection,
    date: Option<&str>,
    user_id: Option<i32>,
    page: i32,
    page_size: i32,
) -> Result<(Vec<EntryLogRow>, i64)> {
    let mut where_clauses: Vec<String> = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(d) = date {
        where_clauses.push(format!("date(timestamp) = ?{}", params.len() + 1));
        params.push(Box::new(d.to_string()));
    }
    if let Some(uid) = user_id {
        where_clauses.push(format!("user_id = ?{}", params.len() + 1));
        params.push(Box::new(uid));
    }

    let where_sql = if where_clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", where_clauses.join(" AND "))
    };

    let count_sql = format!("SELECT COUNT(*) FROM entry_logs {}", where_sql);
    let count_params: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|v| v.as_ref()).collect();
    let total: i64 = db_conn.query_row(&count_sql, count_params.as_slice(), |row| row.get(0))?;

    let offset = (page - 1).max(0) * page_size;
    let query_sql = format!(
        "SELECT id, user_id, auth_method, success, timestamp \
         FROM entry_logs {} ORDER BY id DESC LIMIT ?{} OFFSET ?{}",
        where_sql,
        params.len() + 1,
        params.len() + 2,
    );
    let mut all_params: Vec<Box<dyn rusqlite::types::ToSql>> = params;
    all_params.push(Box::new(page_size));
    all_params.push(Box::new(offset));

    let query_params: Vec<&dyn rusqlite::types::ToSql> = all_params.iter().map(|v| v.as_ref()).collect();
    let mut stmt = db_conn.prepare(&query_sql)?;
    let rows = stmt.query_map(query_params.as_slice(), |row| {
        Ok(EntryLogRow {
            id: row.get(0)?,
            user_id: row.get(1)?,
            auth_method: row.get(2)?,
            success: row.get::<_, i32>(3)? != 0,
            timestamp: row.get(4)?,
        })
    })?
    .collect::<rusqlite::Result<Vec<EntryLogRow>>>()?;

    Ok((rows, total))
}

mod tests {
    use super::*;
    use rand::Rng;

    fn gen_random_name() -> String {
        let mut rng = rand::thread_rng();
        (0..8)
            .map(|_| {
                let idx = rng.gen_range(0..36);
                if idx < 26 {
                    (b'A' + idx as u8) as char
                } else {
                    (b'0' + (idx - 26) as u8) as char
                }
            })
            .collect()
    }

    fn random_hex_id() -> String {
        let mut rng = rand::thread_rng();
        let num: u32 = rng.r#gen();
        format!("{:08X}", num)
    }

    fn random_phone() -> String {
        let mut rng = rand::thread_rng();
        let prefixes = ['3', '4', '5', '6', '7', '8', '9'];
        let prefix = prefixes[rng.gen_range(0..prefixes.len())];
        let suffix: String = (0..9).map(|_| rng.gen_range('0'..='9')).collect();
        format!("1{}{}", prefix, suffix)
    }
    fn gen_random_user_info() -> UserTableInfo {
        UserTableInfo {
            name: gen_random_name(),
            nfc_uid: random_hex_id(),
            phone: random_phone(),
            department: "TestDept".to_string(),
            is_active: 1,
        }
    }
    #[test]
    fn test_db() -> Result<()> {
        let _ = std::fs::remove_file("access_control.db");
        let mut connection = init_db(None)?;
        let test_user_info = UserTableInfo {
            name: "Alice".to_string(),
            nfc_uid: "ABC12345".to_string(),
            phone: "13800138000".to_string(),
            department: "IT".to_string(),
            is_active: 1,
        };
        register_user(&connection, test_user_info)?;
        let mut user_infos = Vec::new();
        for _ in 0..10 {
            user_infos.push(gen_random_user_info());
        }
        register_users(&mut connection, user_infos)?;
        let unlock_result = unlock_nfc(&connection,"ABC12345")?;
        if unlock_result {
            println!("NFC解锁成功");
        } else {
            println!("NFC解锁失败");
        }
        let temp_code = apply_temp_code(&connection, 1, Duration::minutes(5))?;
        println!("临时码: {}", temp_code);
        let temp_code_result = unlock_temp_code(&connection, &temp_code)?;
        if temp_code_result {
            println!("临时码解锁成功");
        } else {            
            println!("临时码解锁失败");
        }
        Ok(())
    }
}