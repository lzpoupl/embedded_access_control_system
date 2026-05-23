use libc::{self, MAP_SHARED, PROT_READ, PROT_WRITE, c_void, close, mmap, munmap};
use std::{fs::{File, OpenOptions}, io, os::unix::io::AsRawFd, ptr};

const MOTOR_ADDR: usize = 0xe2 << 1; // 步进电机硬件地址
const MOTOR_CW: u8 = 3;
const MOTOR_CCW: u8 = 2;
const MOTOR_STOP: u8 = 0;

pub struct Motor {
    cpld: *mut u8,
    mem_fd: File,
}

impl Motor {
    pub fn new() -> io::Result<Self> {
        let mem_fd = OpenOptions::new().read(true).write(true).open("/dev/mem")?;
        let mem_fd_raw = mem_fd.as_raw_fd();

        let cpld = unsafe {
            mmap(
                ptr::null_mut(),
                0x4,
                PROT_READ | PROT_WRITE,
                MAP_SHARED,
                mem_fd_raw,
                0x8000000,
            )
        };

        if cpld == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }

        let cpld = cpld as *mut u8;

        Ok(Motor { cpld, mem_fd })
    }

    pub fn clockwise(&self) {
        unsafe {
            *(self.cpld.add(MOTOR_ADDR)) = MOTOR_CW;
        }
    }

    pub fn counterclockwise(&self) {
        unsafe {
            *(self.cpld.add(MOTOR_ADDR)) = MOTOR_CCW;
        }
    }

    pub fn stop(&self) {
        unsafe {
            *(self.cpld.add(MOTOR_ADDR)) = MOTOR_STOP;
        }
    }
}

impl Drop for Motor {
    fn drop(&mut self) {
        unsafe {
            munmap(self.cpld as *mut c_void, 0x4);
        }
    }
}

unsafe impl Send for Motor {}
unsafe impl Sync for Motor {}
