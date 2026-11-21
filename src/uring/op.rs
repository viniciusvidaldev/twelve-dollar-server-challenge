use std::{marker::PhantomData, os::fd::RawFd};

use crate::{sys::io_uring, uring::buf_ring::BufRing};

#[derive(Clone, Copy)]
pub struct FixedFile(u32);

impl FixedFile {
    pub fn from_accept(res: u32) -> Self {
        Self(res)
    }

    pub fn index(self) -> i32 {
        self.0 as i32
    }

    pub fn close_index(self) -> u32 {
        self.0 + 1
    }
}

pub enum Op {
    Nop,
    Accept {
        fd: RawFd,
    },
    Recv {
        file: FixedFile,
        len: u32,
    },
    Send {
        file: FixedFile,
        buf: *const u8,
        len: u32,
    },
    Close {
        file: FixedFile,
    },
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
                sqe.ioprio = io_uring::IORING_ACCEPT_MULTISHOT;
                sqe.splice_fd_in_or_file_index.file_index = io_uring::IORING_FILE_INDEX_ALLOC;
            }
            Self::Recv { file, len } => {
                sqe.opcode = io_uring::IORING_OP_RECV;
                sqe.fd = file.index();
                sqe.flags |= io_uring::IOSQE_FIXED_FILE;
                sqe.flags |= io_uring::IOSQE_BUFFER_SELECT;
                sqe.addr_or_splice_off_in.addr = 0;
                sqe.len = len;
                sqe.buf_index_or_group.buf_group = BufRing::BGID;
            }
            Self::Send { file, buf, len } => {
                sqe.opcode = io_uring::IORING_OP_SEND;
                sqe.fd = file.index();
                sqe.flags |= io_uring::IOSQE_FIXED_FILE;
                sqe.addr_or_splice_off_in.addr = buf as u64;
                sqe.len = len;
            }
            Self::Close { file } => {
                sqe.opcode = io_uring::IORING_OP_CLOSE;
                sqe.fd = 0;
                sqe.splice_fd_in_or_file_index.file_index = file.close_index();
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
