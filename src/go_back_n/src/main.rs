use std::io::{Read, Write};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use go_back_n::{GbnStream, Interface, packet_loop};

fn main() {
    let ih = Arc::new(Interface::new());
    let mut stream = GbnStream {
        is_left_side: true,
        ih: ih.clone(),
    };
    thread::spawn(move || {
        stream.write(b"Hello World");
        thread::sleep(Duration::from_secs(1));
        stream.write(b"Hello again");
    });
    let mut stream = GbnStream {
        is_left_side: false,
        ih: ih.clone(),
    };
    thread::spawn(move || {
        let mut buf = [0u8; 64];
        loop {
            let nread = stream.read(&mut buf).unwrap();
            println!("{}", String::from_utf8_lossy(&buf[..nread]));
        }
    });
    let jh = thread::spawn(move || packet_loop(ih));
    jh.join();
}