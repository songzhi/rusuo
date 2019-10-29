use std::fmt;

use byteorder::NetworkEndian;
use intbits;
use intbits::Bits;
use zerocopy::{AsBytes, byteorder::{U16, U32}, ByteSlice, FromBytes, LayoutVerified, Unaligned};

#[derive(FromBytes, AsBytes, Unaligned)]
#[repr(C)]
pub struct Header {
    pub seq_num: U32<NetworkEndian>,
    pub flags: U16<NetworkEndian>,
    pub body_len: U32<NetworkEndian>,
}

impl Header {
    pub fn new(seq_num: u32, body_len: u32, is_ack: bool) -> Self {
        let mut flags = 0;
        flags.set_bit(0, is_ack);
        Self {
            seq_num: U32::new(seq_num),
            flags: U16::new(flags),
            body_len: U32::new(body_len),
        }
    }
    pub fn is_ack(&self) -> bool {
        self.flags.get().bit(0)
    }
}

impl fmt::Display for Header {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Packet[{}] {} {}", self.seq_num.get(), ["Normal", "Ack"][self.is_ack() as usize], self.body_len.get())
    }
}

pub struct Packet<B: ByteSlice> {
    pub header: LayoutVerified<B, Header>,
    pub body: B,
}

impl<B: ByteSlice> Packet<B> {
    pub fn parse(bytes: B) -> Option<Self> {
        let (header, body) = LayoutVerified::new_unaligned_from_prefix(bytes)?;
        Some(Self { header, body })
    }
    pub fn get_seq_num(&self) -> u32 {
        self.header.seq_num.get()
    }
    pub fn is_ack(&self) -> bool {
        self.header.is_ack()
    }

    pub fn get_body_len(&self) -> u32 {
        self.header.body_len.get()
    }
}

