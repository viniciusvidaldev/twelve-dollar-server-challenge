use std::fmt;

#[derive(Clone, Copy, Debug)]
pub enum Token {
    Accept,
    Recv(u32),
    Send(u32),
    Close(u32),
}

impl Token {
    pub fn pack(self) -> u64 {
        match self {
            Self::Accept => 0,
            Self::Recv(slot) => (1 << 32) | slot as u64,
            Self::Send(slot) => (2 << 32) | slot as u64,
            Self::Close(slot) => (3 << 32) | slot as u64,
        }
    }

    pub fn unpack(user_data: u64) -> Self {
        let slot = user_data as u32;
        match user_data >> 32 {
            0 => Self::Accept,
            1 => Self::Recv(slot),
            2 => Self::Send(slot),
            3 => Self::Close(slot),
            _ => unreachable!("unknown op kind in user_data"),
        }
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Accept => f.write_str("accept"),
            Self::Recv(slot) => write!(f, "recv[{slot}]"),
            Self::Send(slot) => write!(f, "send[{slot}]"),
            Self::Close(slot) => write!(f, "close[{slot}]"),
        }
    }
}
