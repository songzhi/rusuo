use std::io::{Read, Write};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use go_back_n::{GbnStream, Interface};

fn main() {
    let i = Interface::default();
    let (stream1, stream2) = i.pair();
    let mut stream = stream1;
    thread::spawn(move || {
        stream.write(b"Hello World");
        thread::sleep(Duration::from_secs(1));
        stream.write(b"Hello again");
    });
    let mut stream = stream2;
    thread::spawn(move || {
        let mut buf = [0u8; 64];
        loop {
            let nread = stream.read(&mut buf).unwrap();
            println!("{}", String::from_utf8_lossy(&buf[..nread]));
        }
    });
}