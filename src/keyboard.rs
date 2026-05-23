use std::fs::File;
use std::io::{self, Read};
use std::sync::mpsc::Sender;
use libc::input_event;

const EV_KEY: u16 = 0x01;

const KEY_1: u16 = 2;
const KEY_2: u16 = 3;
const KEY_3: u16 = 4;
const KEY_4: u16 = 5;
const KEY_5: u16 = 6;
const KEY_6: u16 = 7;
const KEY_7: u16 = 8;
const KEY_8: u16 = 9;
const KEY_9: u16 = 10;
const KEY_0: u16 = 11;

const KEY_KP1: u16 = 79;
const KEY_KP2: u16 = 80;
const KEY_KP3: u16 = 81;
const KEY_KP4: u16 = 75;
const KEY_KP5: u16 = 76;
const KEY_KP6: u16 = 77;
const KEY_KP7: u16 = 71;
const KEY_KP8: u16 = 72;
const KEY_KP9: u16 = 73;
const KEY_KP0: u16 = 82;

const KEY_KPASTERISK: u16 = 55;
const KEY_KPENTER: u16 = 96;

pub struct Keyboard {
    file: File,
    sender: Sender<char>,
}

impl Keyboard {
    pub fn new(device_path: &str, sender: Sender<char>) -> io::Result<Self> {
        let file = File::open(device_path)?;
        Ok(Self { file, sender })
    }

    pub fn run(&mut self) -> io::Result<()> {
        let event_size = std::mem::size_of::<input_event>();
        let mut buf = vec![0u8; event_size];

        loop {
            let n = self.file.read(&mut buf)?;
            if n != event_size {
                continue;
            }

            let event: input_event = unsafe { std::ptr::read_unaligned(buf.as_ptr() as *const input_event) };

            if event.type_ != EV_KEY {
                continue;
            }
            if event.value != 1 {
                continue;
            }

            let c = match event.code {
                KEY_KPASTERISK => '*',
                KEY_KPENTER => '#',
                KEY_1 | KEY_KP1 => '1',
                KEY_2 | KEY_KP2 => '2',
                KEY_3 | KEY_KP3 => '3',
                KEY_4 | KEY_KP4 => '4',
                KEY_5 | KEY_KP5 => '5',
                KEY_6 | KEY_KP6 => '6',
                KEY_7 | KEY_KP7 => '7',
                KEY_8 | KEY_KP8 => '8',
                KEY_9 | KEY_KP9 => '9',
                KEY_0 | KEY_KP0 => '0',
                _ => continue,
            };

            if self.sender.send(c).is_err() {
                break;
            }
        }

        Ok(())
    }
}
