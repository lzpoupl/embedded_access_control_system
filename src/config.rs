use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub devices: DevicesConfig,
    pub access: AccessConfig,
    pub web: WebConfig,
    pub database: DatabaseConfig,
    pub display: DisplayConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevicesConfig {
    pub nfc_uart: String,
    pub keyboard_input: String,
    pub buzzer_input: String,
    pub cpld_mem_base: String,
    pub cpld_mem_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessConfig {
    pub lock_open_ms: u64,
    pub temp_code_len: usize,
    pub temp_code_ttl_min: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebConfig {
    pub listen: String,
    pub static_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayConfig {
    pub enabled: bool,
    pub qt_binary: String,
}

pub fn load_config(path: &str) -> Result<Config, crate::error::AppError> {
    let content = fs::read_to_string(Path::new(path)).map_err(|e| {
        crate::error::AppError::Config(format!("无法读取配置文件 {}: {}", path, e))
    })?;
    toml::from_str(&content)
        .map_err(|e| crate::error::AppError::Config(format!("配置文件解析失败: {}", e)))
}

pub fn save_config(path: &str, config: &Config) -> Result<(), crate::error::AppError> {
    let content = toml::to_string_pretty(config)
        .map_err(|e| crate::error::AppError::Config(format!("配置序列化失败: {}", e)))?;
    fs::write(Path::new(path), content)
        .map_err(|e| crate::error::AppError::Config(format!("无法写入配置文件 {}: {}", path, e)))?;
    Ok(())
}
