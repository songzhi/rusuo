use std::cell::RefCell;
use std::io::{Read, Result, Write};
use std::sync::{Arc, Condvar, Mutex};
use std::sync::mpsc::{channel, Receiver};
use std::thread;
use std::time::Duration;

use connection::Connection;
use packet::{Header, Packet};

use crate::connection::PacketWrapper;

pub mod packet;
pub mod connection;


pub struct Interface {
    left: RefCell<Connection>,
    right: RefCell<Connection>,
    rcv_var: Condvar,
    rx: Receiver<PacketWrapper>,
}

impl Interface {
    pub fn get_connection(&self, is_left_side: bool) -> &RefCell<Connection> {
        if is_left_side {
            &self.left
        } else {
            &self.right
        }
    }
    pub fn packet_loop(&mut self) {
        loop {
            if let Ok(packet) = self.rx.recv_timeout(Duration::from_millis(10)) {
                let mut c = self.get_connection(!packet.is_left_side()).borrow_mut();
                c.on_packet(packet.unwrap());
            } else {
                self.left.borrow_mut().on_tick().unwrap();
                self.right.borrow_mut().on_tick().unwrap();
            }
        }
    }
}

pub struct GbnStream {
    h: Arc<Interface>,
    is_left_side: bool,
}

impl Write for GbnStream {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let mut c = self.h.get_connection(self.is_left_side).borrow_mut();
        c.unsent.extend(buf.iter());
        Ok(buf.len())
    }
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

impl Read for GbnStream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let mut c = self.h.get_connection(self.is_left_side).borrow_mut();
        let cm = Mutex::new(false);
        let mut cm = cm.lock().unwrap();
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

            cm = self.h.rcv_var.wait(cm).unwrap();
        }
    }
}