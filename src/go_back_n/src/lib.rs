use std::sync::mpsc::channel;
use std::thread;

use connection::Connection;
use packet::{Header, Packet};

pub mod packet;
pub mod connection;

fn packet_loop() {
    let (tx1, rx1) = channel();
    let (tx2, rx2) = channel();
}