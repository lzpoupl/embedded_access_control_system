use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

use crate::config::Config;
use crate::db;
use crate::error::AppError;

#[derive(Clone)]
pub struct WebState {
    pub config_path: String,
    pub config: Arc<std::sync::RwLock<Config>>,
    pub db_conn: Arc<Mutex<Connection>>
}

pub fn router(state: WebState) -> Router {
    let serve_dir = ServeDir::new(&state.config.read().unwrap().web.static_dir);

    Router::new()
        .route("/api/users", get(list_users).post(create_user))
        .route("/api/users/{id}", get(get_user).put(update_user).delete(delete_user))
        .route("/api/users/{id}/temp-code", post(generate_temp_code))
        .route("/api/entry-logs", get(list_entry_logs_handler))
        .route("/api/config", get(get_config).put(update_config))
        .fallback_service(serve_dir)
        .layer(CorsLayer::permissive())
        .with_state(state)
}

pub async fn run_server(state: WebState) -> Result<(), AppError> {
    let listen = state.config.read().unwrap().web.listen.clone();
    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .map_err(|e| AppError::Config(format!("无法绑定 {}: {}", listen, e)))?;
    axum::serve(listener, router(state))
        .await
        .map_err(|e| AppError::Config(format!("服务器启动失败: {}", e)))?;
    Ok(())
}

// ---------- 请求/响应类型 ----------

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub name: String,
    pub nfc_uid: Option<String>,
    pub phone: Option<String>,
    pub department: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    pub name: Option<String>,
    pub nfc_uid: Option<String>,
    pub phone: Option<String>,
    pub department: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: i32,
    pub name: String,
    pub nfc_uid: Option<String>,
    pub phone: Option<String>,
    pub department: Option<String>,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl From<db::UserRow> for UserResponse {
    fn from(row: db::UserRow) -> Self {
        UserResponse {
            id: row.id,
            name: row.name,
            nfc_uid: row.nfc_uid,
            phone: row.phone,
            department: row.department,
            is_active: row.is_active,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct TempCodeResponse {
    pub code: String,
    pub expires_at: String,
}

#[derive(Debug, Serialize)]
pub struct EntryLogResponse {
    pub id: i32,
    pub user_id: Option<i32>,
    pub auth_method: String,
    pub success: bool,
    pub timestamp: String,
}

impl From<db::EntryLogRow> for EntryLogResponse {
    fn from(row: db::EntryLogRow) -> Self {
        EntryLogResponse {
            id: row.id,
            user_id: row.user_id,
            auth_method: row.auth_method,
            success: row.success,
            timestamp: row.timestamp,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct EntryLogListResponse {
    pub data: Vec<EntryLogResponse>,
    pub total: i64,
    pub page: i32,
    pub page_size: i32,
}

#[derive(Debug, Deserialize)]
pub struct EntryLogQuery {
    pub date: Option<String>,
    pub user_id: Option<i32>,
    pub page: Option<i32>,
    pub page_size: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct UserListQuery {
    pub search: Option<String>,
}

// ---------- 人员管理 ----------

async fn create_user(
    State(state): State<WebState>,
    Json(req): Json<CreateUserRequest>,
) -> Result<(StatusCode, Json<UserResponse>), AppError> {
    if req.name.trim().is_empty() {
        return Err(AppError::BadRequest("姓名不能为空".into()));
    }
    let user_info = db::UserTableInfo::new(
        req.name,
        req.nfc_uid.unwrap_or_default(),
        req.phone.unwrap_or_default(),
        req.department.unwrap_or_default(),
        1,
    );
    let db_conn = state.db_conn.lock().unwrap();
    db::register_user(&db_conn, user_info)?;

    let users = db::list_users(&db_conn, None)?;
    let created = users.into_iter().next().ok_or_else(|| {
        AppError::Internal("创建用户后无法查询".into())
    })?;

    Ok((StatusCode::CREATED, Json(created.into())))
}

async fn list_users(
    State(state): State<WebState>,
    Query(query): Query<UserListQuery>,
) -> Result<Json<Vec<UserResponse>>, AppError> {
    let db_conn = state.db_conn.lock().unwrap();
    let users = db::list_users(&db_conn, query.search.as_deref())?;
    Ok(Json(users.into_iter().map(UserResponse::from).collect()))
}

async fn get_user(
    State(state): State<WebState>,
    Path(id): Path<i32>,
) -> Result<Json<UserResponse>, AppError> {
    let db_conn = state.db_conn.lock().unwrap();
    let user = db::get_user_by_id(&db_conn, id)?
        .ok_or_else(|| AppError::NotFound(format!("用户 {} 不存在", id)))?;
    Ok(Json(user.into()))
}

async fn update_user(
    State(state): State<WebState>,
    Path(id): Path<i32>,
    Json(req): Json<UpdateUserRequest>,
) -> Result<Json<UserResponse>, AppError> {
    let updates = db::UserUpdate {
        name: req.name,
        nfc_uid: req.nfc_uid,
        phone: req.phone,
        department: req.department,
        is_active: req.is_active,
    };
    let db_conn = state.db_conn.lock().unwrap();
    let affected = db::update_user(&db_conn, id, &updates)?;
    if !affected {
        return Err(AppError::NotFound(format!("用户 {} 不存在", id)));
    }
    let user = db::get_user_by_id(&db_conn, id)?
        .ok_or_else(|| AppError::NotFound(format!("用户 {} 不存在", id)))?;
    Ok(Json(user.into()))
}

async fn delete_user(
    State(state): State<WebState>,
    Path(id): Path<i32>,
) -> Result<StatusCode, AppError> {
    let db_conn = state.db_conn.lock().unwrap();
    let affected = db::delete_user(&db_conn, id)?;
    if !affected {
        return Err(AppError::NotFound(format!("用户 {} 不存在或已停用", id)));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn generate_temp_code(
    State(state): State<WebState>,
    Path(id): Path<i32>,
) -> Result<Json<TempCodeResponse>, AppError> {
    let db_conn = state.db_conn.lock().unwrap();
    let user = db::get_user_by_id(&db_conn, id)?
        .ok_or_else(|| AppError::NotFound(format!("用户 {} 不存在", id)))?;
    if !user.is_active {
        return Err(AppError::BadRequest("用户已停用".into()));
    }
    let ttl_min = state.config.read().unwrap().access.temp_code_ttl_min;
    let code = db::apply_temp_code(&db_conn, id, chrono::Duration::minutes(ttl_min))?;
    let expires_at = chrono::Local::now() + chrono::Duration::minutes(ttl_min);
    Ok(Json(TempCodeResponse {
        code,
        expires_at: expires_at.format("%Y-%m-%d %H:%M:%S").to_string(),
    }))
}

// ---------- 记录查询 ----------

async fn list_entry_logs_handler(
    State(state): State<WebState>,
    Query(query): Query<EntryLogQuery>,
) -> Result<Json<EntryLogListResponse>, AppError> {
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).max(1).min(100);
    let db_conn = state.db_conn.lock().unwrap();
    let (rows, total) = db::list_entry_logs(
        &db_conn,
        query.date.as_deref(),
        query.user_id,
        page,
        page_size,
    )?;
    Ok(Json(EntryLogListResponse {
        data: rows.into_iter().map(EntryLogResponse::from).collect(),
        total,
        page,
        page_size,
    }))
}

// ---------- 系统配置 ----------

async fn get_config(
    State(state): State<WebState>,
) -> Json<Config> {
    Json(state.config.read().unwrap().clone())
}

async fn update_config(
    State(state): State<WebState>,
    Json(new_config): Json<Config>,
) -> Result<StatusCode, AppError> {
    crate::config::save_config(&state.config_path, &new_config)?;
    let mut cfg = state.config.write().unwrap();
    *cfg = new_config;
    Ok(StatusCode::NO_CONTENT)
}
