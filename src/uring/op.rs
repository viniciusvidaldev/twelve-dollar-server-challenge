use std::{marker::PhantomData, os::fd::RawFd};

use crate::sys::{IORING_OP_NOP, IORING_OP_READ, IORING_OP_WRITE, io_uring_sqe};

const NO_OFFSET: u64 = u64::MAX;

pub enum Op {
    Nop,
    Read {
        fd: RawFd,
        buf: *mut u8,
        len: u32,
        offset: Option<u64>,
    },
    Write {
        fd: RawFd,
        buf: *const u8,
        len: u32,
        offset: Option<u64>,
    },
}

impl Op {
    pub fn encode(&self) -> Entry<Untagged> {
        let mut sqe = io_uring_sqe::default();
        match *self {
            Self::Nop => {
                sqe.opcode = IORING_OP_NOP;
            }
            Self::Read {
                fd,
                buf,
                len,
                offset,
            } => {
                sqe.opcode = IORING_OP_READ;
                sqe.fd = fd;
                sqe.addr_or_splice_off_in.addr = buf as u64;
                sqe.len = len;
                sqe.off_or_addr2.off = offset.unwrap_or(NO_OFFSET);
            }
            Self::Write {
                fd,
                buf,
                len,
                offset,
            } => {
                sqe.opcode = IORING_OP_WRITE;
                sqe.fd = fd;
                sqe.addr_or_splice_off_in.addr = buf as u64;
                sqe.len = len;
                sqe.off_or_addr2.off = offset.unwrap_or(NO_OFFSET);
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
    pub(crate) sqe: io_uring_sqe,
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
