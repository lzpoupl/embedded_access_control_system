use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use crate::motor::Motor;

pub enum AuthEvent {
    NfcUid(String),
    KeyChar(char),
}

pub struct Auth {
    rx: Receiver<AuthEvent>,
    motor: Arc<Motor>,
    lock_open_ms: u64,
}

impl Auth {
    pub fn new(rx: Receiver<AuthEvent>, motor: Arc<Motor>, lock_open_ms: u64) -> Self {
        Self { rx, motor, lock_open_ms }
    }

    pub fn run(&mut self) {
        let mut buf = String::new();

        while let Ok(event) = self.rx.recv() {
            match event {
                AuthEvent::NfcUid(uid) => {
                    self.display_submit();
                    self.handle_nfc(&uid);
                }
                AuthEvent::KeyChar(c) => {
                    self.handle_key(c, &mut buf);
                }
            }
        }
    }

    fn handle_key(&self, c: char, buf: &mut String) {
        match c {
            '*' => {
                buf.clear();
                self.display_clear();
            }
            '#' => {
                if buf.is_empty() {
                    return;
                }
                self.display_submit();
                let code = buf.clone();
                buf.clear();
                self.handle_code(&code);
            }
            _ => {
                buf.push(c);
                self.display_key_press(c);
            }
        }
    }

    fn handle_nfc(&self, uid: &str) {
        match self.db_find_user_by_nfc_uid(uid) {
            Some((user_id, name)) => self.grant(user_id, &name, "nfc"),
            None => self.deny(None),
        }
    }

    fn handle_code(&self, code: &str) {
        match self.db_verify_temp_code(code) {
            Some((user_id, name)) => {
                self.db_delete_temp_code(code);
                self.grant(user_id, &name, "temp_code");
            }
            None => self.deny(None),
        }
    }

    fn grant(&self, user_id: i64, name: &str, method: &str) {
        self.motor.counterclockwise();
        self.buzzer_success();
        self.display_auth_ok(name);
        self.db_insert_log(Some(user_id), method, true);
        thread::sleep(Duration::from_millis(self.lock_open_ms));
        self.motor.clockwise();
        self.motor.stop();
        self.display_idle();
    }

    fn deny(&self, user_id: Option<i64>) {
        self.buzzer_failure();
        self.display_auth_fail();
        self.db_insert_log(user_id, "unknown", false);
        thread::sleep(Duration::from_millis(self.lock_open_ms));
        self.display_idle();
    }

    fn db_find_user_by_nfc_uid(&self, _uid: &str) -> Option<(i64, String)> { None }
    fn db_verify_temp_code(&self, _code: &str) -> Option<(i64, String)> { None }
    fn db_delete_temp_code(&self, _code: &str) {}
    fn db_insert_log(&self, _user_id: Option<i64>, _method: &str, _success: bool) {}

    fn display_submit(&self) {}
    fn display_clear(&self) {}
    fn display_key_press(&self, _c: char) {}
    fn display_auth_ok(&self, _name: &str) {}
    fn display_auth_fail(&self) {}
    fn display_idle(&self) {}

    fn buzzer_success(&self) {}
    fn buzzer_failure(&self) {}
}
