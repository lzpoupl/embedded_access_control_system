use libc::{
    self, B115200, BRKINT, CLOCAL, CREAD, CS8, ECHO, ECHOE, ICANON, ICRNL, INPCK, ISIG, ISTRIP,
    IXON, O_NOCTTY, O_RDWR, OPOST, TCSANOW, VMIN, VTIME, cfsetispeed, cfsetospeed, close, open,
    read, tcgetattr, tcsetattr, termios, write,
};
use std::io;
use std::sync::mpsc::Sender;

const WAKEUP: [u8; 24] = [
    0x55, 0x55, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0xff, 0x03, 0xfd, 0xd4, 0x14, 0x01, 0x17, 0x00,
];

const GET_UID: [u8; 11] = [
    0x00, 0x00, 0xFF, 0x04, 0xFC, 0xD4, 0x4A, 0x01, 0x00, 0xE1, 0x00,
];

const UID_WINDOW: usize = 25;

pub struct NfcReader {
    fd: i32,
    sender: Sender<String>,
    last_uid: String,
}

impl NfcReader {
    pub fn new(uart_path: &str, sender: Sender<String>) -> io::Result<Self> {
        let fd = unsafe { open(uart_path.as_ptr() as *const libc::c_char, O_RDWR | O_NOCTTY) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }

        unsafe {
            let mut options: termios = std::mem::zeroed();
            if tcgetattr(fd, &mut options) != 0 {
                close(fd);
                return Err(io::Error::last_os_error());
            }
            cfsetispeed(&mut options, B115200);
            cfsetospeed(&mut options, B115200);
            options.c_cflag |= CLOCAL | CREAD | CS8;
            options.c_lflag &= !(ICANON | ECHO | ECHOE | ISIG);
            options.c_oflag &= !OPOST;
            options.c_iflag &= !(BRKINT | ICRNL | INPCK | ISTRIP | IXON);
            options.c_cc[VTIME] = 0;
            options.c_cc[VMIN] = 1;
            if tcsetattr(fd, TCSANOW, &mut options) != 0 {
                close(fd);
                return Err(io::Error::last_os_error());
            }
        }

        let mut reader = Self { fd, sender, last_uid: String::new() };
        reader.wakeup()?; // 初始化时完成唤醒，只执行一次
        Ok(reader)
    }

    fn wakeup(&mut self) -> io::Result<()> {
        if unsafe {
            write(
                self.fd,
                WAKEUP.as_ptr() as *const libc::c_void,
                WAKEUP.len(),
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }

        let mut buf: Vec<u8> = Vec::new();
        let mut byte: u8 = 0;
        loop {
            let n = unsafe { read(self.fd, &mut byte as *mut u8 as *mut libc::c_void, 1) };
            if n < 0 {
                return Err(io::Error::last_os_error());
            }
            if n == 0 {
                continue;
            }
            buf.push(byte);

            // 用 windows(2) 扫描累积的字节流，寻找 0xD5 0x15 序列
            if buf.get(24-3..=24-2) == Some(&[0xD5, 0x15]) {
                break;
            }
        }

        // ACK 收到，发送 getUID 开始轮询
        if unsafe {
            write(
                self.fd,
                GET_UID.as_ptr() as *const libc::c_void,
                GET_UID.len(),
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn run(&mut self) -> io::Result<()> {
        let mut buf: [u8; UID_WINDOW] = [0u8; UID_WINDOW];
        let mut byte: u8 = 0;

        loop {
            let n = unsafe { read(self.fd, &mut byte as *mut u8 as *mut libc::c_void, 1) };
            if n < 0 {
                return Err(io::Error::last_os_error());
            }
            if n == 0 {
                continue;
            }

            // shift buffer left and append new byte
            for i in 0..(UID_WINDOW - 1) {
                buf[i] = buf[i + 1];
            }
            buf[UID_WINDOW - 1] = byte;

            if is_uid_response(&buf) {
                let uid: u32 = (buf[19] as u32) << 24
                    | (buf[20] as u32) << 16
                    | (buf[21] as u32) << 8
                    | (buf[22] as u32);

                let uid_str = format!("{:08X}", uid);
                if uid_str != self.last_uid {
                    self.last_uid = uid_str.clone();
    
                    if self.sender.send(uid_str).is_err() {
                        break;
                    }
                }

                if unsafe {
                    write(
                        self.fd,
                        GET_UID.as_ptr() as *const libc::c_void,
                        GET_UID.len(),
                    )
                } < 0
                {
                    return Err(io::Error::last_os_error());
                }
            }
        }
        Ok(())
    }
}

fn is_uid_response(buf: &[u8; UID_WINDOW]) -> bool {
    matches!(
        *buf,
        [
            0x00,
            0x00,
            0xFF,
            0x00,
            0xFF,
            0x00,
            0x00,
            0x00,
            0xFF, //  header
            _,
            _,
            _,    //  padding
            0x4B, //  byte 12
            _,
            _,
            _,
            _,
            _,    //  padding
            0x04, //  byte 18
            _,
            _,
            _,
            _,
            _,    //  UID 4B + padding
            0x00, //  terminator
        ]
    )
}

impl Drop for NfcReader {
    fn drop(&mut self) {
        unsafe {
            close(self.fd);
        }
    }
}
