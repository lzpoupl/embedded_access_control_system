use std::env;
use std::fs::File;
use std::io::{self, Read};
use libc::input_event;

mod db;

const EV_KEY: u16 = 0x01; // 手动定义 EV_KEY

fn main() -> io::Result<()> {
    // 获取命令行参数
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {} <input_device>", args[0]);
        eprintln!("Example: {} /dev/input/event0", args[0]);
        std::process::exit(1);
    }

    let device_path = &args[1];

    // 打开输入设备文件
    let mut file = File::open(device_path)?;
    println!("Listening for key events on {}...", device_path);

    // 读取输入事件
    let mut event: input_event = unsafe { std::mem::zeroed() };
    let event_size = std::mem::size_of::<input_event>();

    loop {
        // 读取事件
        let bytes_read = file.read(unsafe {
            std::slice::from_raw_parts_mut(
                &mut event as *mut _ as *mut u8,
                event_size,
            )
        })?;

        if bytes_read != event_size {
            eprintln!("Failed to read complete event");
            continue;
        }

        // 处理按键事件
        if event.type_ == EV_KEY {
            if event.value == 0 || event.value == 1 {
                let key_state = if event.value == 1 { "Pressed" } else { "Released" };
                println!("key {} {}", event.code, key_state);
            }
        }
    }
}
