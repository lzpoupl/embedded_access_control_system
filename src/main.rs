use std::sync::{Arc, Mutex, RwLock, mpsc};

mod auth;
mod config;
mod db;
mod error;
mod keyboard;
mod motor;
mod nfc;
mod web;

#[tokio::main]
async fn main() {
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".to_string());

    let config = config::load_config(&config_path).expect("加载配置文件失败");

    let db_conn = Arc::new(Mutex::new(
        db::init_db(Some(&config.database.path)).expect("数据库初始化失败"),
    ));

    let motor = Arc::new(motor::Motor::new().expect("电机初始化失败"));

    let (key_tx, key_rx) = mpsc::channel::<char>();
    let (nfc_tx, nfc_rx) = mpsc::channel::<String>();
    let (auth_tx, auth_rx) = mpsc::channel::<auth::AuthEvent>();

    let auth_tx_key = auth_tx.clone();
    std::thread::spawn(move || {
        while let Ok(c) = key_rx.recv() {
            if auth_tx_key.send(auth::AuthEvent::KeyChar(c)).is_err() {
                break;
            }
        }
    });

    std::thread::spawn(move || {
        while let Ok(uid) = nfc_rx.recv() {
            if auth_tx.send(auth::AuthEvent::NfcUid(uid)).is_err() {
                break;
            }
        }
    });

    let web_state = web::WebState {
        config_path: config_path.clone(),
        config: Arc::new(RwLock::new(config.clone())),
        db_conn: db_conn.clone(),
    };

    let key_dev = config.devices.keyboard_input.clone();
    tokio::task::spawn_blocking(move || {
        let mut kb =
            keyboard::Keyboard::new(&key_dev, key_tx).expect("键盘设备打开失败");
        if let Err(e) = kb.run() {
            eprintln!("键盘错误: {}", e);
        }
    });

    let nfc_dev = config.devices.nfc_uart.clone();
    tokio::task::spawn_blocking(move || {
        let mut reader =
            nfc::NfcReader::new(&nfc_dev, nfc_tx).expect("NFC 设备打开失败");
        if let Err(e) = reader.run() {
            eprintln!("NFC 错误: {}", e);
        }
    });

    let lock_ms = config.access.lock_open_ms;
    tokio::task::spawn_blocking(move || {
        let mut auth_svc = auth::Auth::new(auth_rx, motor, lock_ms);
        auth_svc.run();
    });

    let web_handle = tokio::spawn(async {
        if let Err(e) = web::run_server(web_state).await {
            eprintln!("Web 服务器错误: {:?}", e);
        }
    });

    tokio::signal::ctrl_c().await.ok();
    println!("正在关闭…");
    web_handle.abort();
}
