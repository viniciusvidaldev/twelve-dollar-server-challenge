use std::{marker::PhantomData, os::fd::RawFd};

use crate::sys::io_uring;

pub enum Op {
    Nop,
    Accept { fd: RawFd },
    Recv { fd: RawFd, buf: *mut u8, len: u32 },
    Send { fd: RawFd, buf: *const u8, len: u32 },
    Close { fd: RawFd },
}

impl Op {
    pub fn encode(&self) -> Entry<Untagged> {
        let mut sqe = io_uring::io_uring_sqe::default();
        match *self {
            Self::Nop => {
                sqe.opcode = io_uring::IORING_OP_NOP;
                sqe.fd = -1;
            }
            Self::Accept { fd } => {
                sqe.opcode = io_uring::IORING_OP_ACCEPT;
                sqe.fd = fd;
            }
            Self::Recv { fd, buf, len } => {
                sqe.opcode = io_uring::IORING_OP_RECV;
                sqe.fd = fd;
                sqe.addr_or_splice_off_in.addr = buf as u64;
                sqe.len = len;
            }
            Self::Send { fd, buf, len } => {
                sqe.opcode = io_uring::IORING_OP_SEND;
                sqe.fd = fd;
                sqe.addr_or_splice_off_in.addr = buf as u64;
                sqe.len = len;
            }
            Self::Close { fd } => {
                sqe.opcode = io_uring::IORING_OP_CLOSE;
                sqe.fd = fd;
            }
        }
        Entry {
            sqe,
            _state: PhantomData,
        }
    }
}

#[derive(Debug)]
pub struct Tagged;
#[derive(Debug)]
pub struct Untagged;

#[must_use]
pub struct Entry<T> {
    pub(crate) sqe: io_uring::io_uring_sqe,
    _state: PhantomData<T>,
}

impl<S> Entry<S> {
    pub fn flags(mut self, flags: u8) -> Self {
        self.sqe.flags = flags;
        self
    }
}

impl Entry<Untagged> {
    pub fn user_data(mut self, user_data: u64) -> Entry<Tagged> {
        self.sqe.user_data = user_data;
        Entry {
            sqe: self.sqe,
            _state: PhantomData,
        }
    }
}
