use std::cmp::{min, Ordering};
use std::collections::VecDeque;
use std::io::Result;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use log::trace;
use zerocopy::AsBytes;

use super::packet::{Header, Packet};

pub const TIMEOUT_DURATION: Duration = Duration::from_secs(3);

pub enum PacketWrapper {
    Left(Box<[u8]>),
    Right(Box<[u8]>),
}

impl PacketWrapper {
    pub fn new(data: Box<[u8]>, is_left_side: bool) -> Self {
        if is_left_side {
            PacketWrapper::Left(data)
        } else {
            PacketWrapper::Right(data)
        }
    }
    pub fn is_left_side(&self) -> bool {
        match self {
            PacketWrapper::Left(_) => true,
            PacketWrapper::Right(_) => false
        }
    }
    pub fn unwrap(self) -> Box<[u8]> {
        match self {
            PacketWrapper::Left(data) => data,
            PacketWrapper::Right(data) => data
        }
    }
}

pub struct Connection {
    send: SendSequenceSpace,
    recv: RecvSequenceSpace,
    pub(crate) incoming: VecDeque<u8>,
    timer: Option<Instant>,
    unacked: VecDeque<Box<[u8]>>,
    pub(crate)unsent: VecDeque<u8>,

    is_left_side: bool,
    tx: Sender<PacketWrapper>,
}


impl Connection {
    pub const MAX_BODY_SIZE: u32 = 1024;
    pub fn new(is_left_side: bool, tx: Sender<PacketWrapper>) -> Self {
        Self {
            send: SendSequenceSpace::new(1),
            recv: RecvSequenceSpace::new(1),
            incoming: VecDeque::new(),
            timer: None,
            unacked: VecDeque::new(),
            unsent: VecDeque::new(),
            is_left_side,
            tx,
        }
    }

    pub fn on_tick(&mut self) -> Result<()> {
        self.send_if_could();
        if let Some(timeout) = self.timer {
            if timeout <= Instant::now() {
                self.reset_timer();
                for packet in self.unacked.iter() {
                    trace!("Connection[{}]: Resend {}", self.is_left_side as usize, Packet::parse(packet.as_ref()).unwrap().header);
                    self.tx.send(PacketWrapper::new(packet.clone(), self.is_left_side)).expect("Send failed");
                };
            }
        }
        Ok(())
    }
    #[inline]
    fn reset_timer(&mut self) {
        self.timer = Some(Instant::now() + TIMEOUT_DURATION);
    }

    pub fn send_if_could(&mut self) {
        if self.send.is_sendable() && !self.unsent.is_empty() {
            let body_len = min(Self::MAX_BODY_SIZE as usize, self.unsent.len());
            let header = Header::new(self.send.get_next_seq_num_then_inc(), body_len as u32, false);
            trace!("Connection[{}]: Send {}", self.is_left_side as usize, header);
            let packet = header.as_bytes().iter().copied().chain(self.unsent.drain(..body_len)).collect::<Box<_>>();
            self.tx.send(PacketWrapper::new(packet.clone(), self.is_left_side)).unwrap();
            self.unacked.push_back(packet);
            self.reset_timer();
        }
    }

    pub fn on_packet(&mut self, packet: Box<[u8]>) {
        if let Some(packet) = Packet::parse(packet.as_ref()) {
            trace!("Connection[{}]: Recv {}", self.is_left_side as usize, packet.header);
            if packet.is_ack() {
                let acked_count = self.send.ack(packet.get_seq_num());
                drop(self.unacked.drain(..acked_count));
                if acked_count != 0 {
                    self.reset_timer();
                }
            } else if let Some(is_fresh) = self.recv.rcv(packet.get_seq_num()) {
                let ack_header = Header::new(packet.get_seq_num(), 0, true);
                trace!("Connection[{}]: Send {}", self.is_left_side as usize, ack_header);
                let ack_packet = PacketWrapper::new(ack_header.as_bytes().iter().copied().collect::<Box<_>>(), self.is_left_side);
                self.tx.send(ack_packet).expect("Send ACK failed");
                if is_fresh {
                    self.incoming.extend(packet.body.iter().take(packet.get_body_len() as usize));
                }
            }
        }
    }
}


pub struct SendSequenceSpace {
    // 最早的未确认分组的序号
    pub base: u32,
    pub next_seq_num: u32, // 最小的未使用序号
}

impl SendSequenceSpace {
    const N: u32 = 32;
    #[inline]
    pub fn new(base: u32) -> Self {
        Self {
            base,
            next_seq_num: base,
        }
    }
    #[inline]
    pub fn is_sendable(&self) -> bool {
        wrapping_lt(self.next_seq_num, self.base.wrapping_add(Self::N - 1))
    }
    #[inline]
    pub fn ack(&mut self, seq_num: u32) -> usize {
        if wrapping_lt(self.base, seq_num) {
            0
        } else {
            let acked_count = seq_num.wrapping_sub(self.base).wrapping_add(1);
            self.base = self.base.wrapping_add(acked_count);
            acked_count as usize
        }
    }
    #[inline]
    pub fn unacked_count(&self) -> usize {
        self.next_seq_num.wrapping_sub(self.base) as usize
    }
    #[inline]
    pub fn has_unacked(&self) -> bool {
        self.unacked_count() > 0
    }
    #[inline]
    pub fn get_next_seq_num_then_inc(&mut self) -> u32 {
        let old = self.next_seq_num;
        self.next_seq_num = old.wrapping_add(1);
        old
    }
}

pub struct RecvSequenceSpace {
    pub expected_seq_num: u32
}

impl RecvSequenceSpace {
    #[inline]
    pub fn new(expected_seq_num: u32) -> Self {
        Self {
            expected_seq_num
        }
    }
    /// 返回值None代表当前无法接受的包,Some(false)代表已经接受过的包
    #[inline]
    pub fn rcv(&mut self, seq_num: u32) -> Option<bool> {
        match self.expected_seq_num.cmp(&seq_num) {
            Ordering::Equal => {
                self.expected_seq_num += 1;
                Some(true)
            }
            Ordering::Greater => Some(false),
            Ordering::Less => None
        }
    }
}

#[inline]
fn wrapping_lt(lhs: u32, rhs: u32) -> bool {
//     From RFC1323:
//         TCP determines if a data segment is "old" or "new" by testing
//         whether its sequence number is within 2**31 bytes of the left edge
//         of the window, and if it is not, discarding the data as "old".  To
//         insure that new data is never mistakenly considered old and vice-
//         versa, the left edge of the sender's window has to be at most
//         2**31 away from the right edge of the receiver's window.
    lhs.wrapping_sub(rhs) > (1 << 31)
}