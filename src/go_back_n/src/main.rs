use std::io::{Read, Write};
use std::thread;
use std::time::Duration;

use log::info;

use go_back_n::Interface;

fn main() {
    pretty_env_logger::init();
    let i = Interface::default();
    let (stream1, stream2) = i.pair();
    let mut stream = stream1;
    thread::spawn(move || {
        stream.write_all(b"Hello World").unwrap();
        thread::sleep(Duration::from_secs(1));
        stream.write_all(b"Hello again").unwrap();
    });
    let mut stream = stream2;
    thread::spawn(move || {
        let mut buf = [0u8; 64];
        loop {
            let nread = stream.read(&mut buf).unwrap();
            info!("{}", String::from_utf8_lossy(&buf[..nread]));
        }
    });
}