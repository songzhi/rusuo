use std::io::{Read, Result, Write};
use std::sync::{Arc, Condvar, Mutex};
use std::sync::mpsc::{channel, Receiver};
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

use rand::random;

use connection::Connection;
use packet::Packet;

use crate::connection::PacketWrapper;

pub mod packet;
pub mod connection;

type InterfaceHandle = Arc<FooBar>;

fn packet_loop(ih: InterfaceHandle) {
    loop {
        if let Ok(packet) = ih.rx.lock().unwrap().recv_timeout(Duration::from_millis(10)) {
            let is_left_side = packet.is_left_side();
            let packet = packet.unwrap();
            if random::<u8>() > 200 {
                println!("Loop: Ignored {} from Connection[{}]", Packet::parse(packet.as_ref()).unwrap().header, is_left_side as usize);
                continue;
            }
            let mut c = ih.get_connection(!is_left_side).lock().unwrap();
            c.on_packet(packet);
            if !c.incoming.is_empty() {
                ih.rcv_var.notify_all();
            }
        } else {
            ih.left.lock().unwrap().on_tick().unwrap();
            ih.right.lock().unwrap().on_tick().unwrap();
        }
    }
}

struct FooBar {
    left: Mutex<Connection>,
    right: Mutex<Connection>,
    rcv_var: Condvar,
    rx: Mutex<Receiver<PacketWrapper>>,
}

impl Default for FooBar {
    fn default() -> Self {
        let (tx, rx) = channel();
        let left = Mutex::new(Connection::new(true, tx.clone()));
        let right = Mutex::new(Connection::new(false, tx));
        Self {
            left,
            right,
            rcv_var: Condvar::new(),
            rx: Mutex::new(rx),
        }
    }
}

impl FooBar {
    fn get_connection(&self, is_left_side: bool) -> &Mutex<Connection> {
        if is_left_side {
            &self.left
        } else {
            &self.right
        }
    }
}

pub struct Interface {
    ih: Option<InterfaceHandle>,
    jh: Option<JoinHandle<()>>,
}

impl Drop for Interface {
    fn drop(&mut self) {
        drop(self.ih.take());
        self.jh
            .take()
            .expect("interface dropped more than once")
            .join()
            .unwrap();
    }
}

impl Default for Interface {
    fn default() -> Self {
        let ih = InterfaceHandle::default();
        let jh = {
            let ih = ih.clone();
            thread::spawn(move || packet_loop(ih))
        };
        Self {
            ih: Some(ih),
            jh: Some(jh),
        }
    }
}

impl Interface {
    pub fn pair(&self) -> (GbnStream, GbnStream) {
        (GbnStream {
            is_left_side: true,
            ih: self.ih.clone().unwrap(),
        }, GbnStream {
            is_left_side: false,
            ih: self.ih.clone().unwrap(),
        })
    }
}

pub struct GbnStream {
    ih: InterfaceHandle,
    is_left_side: bool,
}

impl Write for GbnStream {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let mut c = self.ih.get_connection(self.is_left_side).lock().unwrap();
        c.unsent.extend(buf.iter());
        Ok(buf.len())
    }
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

impl Read for GbnStream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let mut c = self.ih.get_connection(self.is_left_side).lock().unwrap();
        loop {
            if !c.incoming.is_empty() {
                let mut nread = 0;
                let (head, tail) = c.incoming.as_slices();
                let hread = std::cmp::min(buf.len(), head.len());
                buf[..hread].copy_from_slice(&head[..hread]);
                nread += hread;
                let tread = std::cmp::min(buf.len() - nread, tail.len());
                buf[hread..(hread + tread)].copy_from_slice(&tail[..tread]);
                nread += tread;
                drop(c.incoming.drain(..nread));
                return Ok(nread);
            }
            c = self.ih.rcv_var.wait(c).unwrap();
        }
    }
}