use std::os::fd::AsRawFd;

use crate::{
    server::{Server, token::Token},
    uring::{Entry, Op, SqFull, Tagged},
};

impl Server {
    pub(super) fn submit_accept(&mut self) {
        let op = Op::Accept {
            fd: self.listener.as_raw_fd(),
        };
        let entry = op.encode().user_data(Token::Accept.pack());
        self.push(entry)
    }

    pub(super) fn submit_recv(&mut self, slot: u32) {
        let conn = self.conns.get_mut(slot);
        let fd = conn.fd();
        let buf = conn.recv_buf();
        let op = Op::Recv {
            fd,
            buf: buf.as_mut_ptr(),
            len: buf.len() as u32,
        };
        let entry = op.encode().user_data(Token::Recv(slot).pack());
        self.push(entry)
    }

    pub(super) fn submit_send(&mut self, slot: u32) {
        let conn = self.conns.get_mut(slot);
        let fd = conn.fd();
        let buf = conn.unsent();
        let op = Op::Send {
            fd,
            buf: buf.as_ptr(),
            len: buf.len() as u32,
        };
        let entry = op.encode().user_data(Token::Send(slot).pack());
        self.push(entry)
    }

    pub(super) fn submit_close(&mut self, slot: u32) {
        let conn = self.conns.get_mut(slot);
        let fd = conn.fd();
        let op = Op::Close { fd };
        let entry = op.encode().user_data(Token::Close(slot).pack());
        self.push(entry)
    }

    fn push(&mut self, mut entry: Entry<Tagged>) {
        loop {
            match unsafe { self.ring.push(entry) } {
                Ok(_) => return,
                Err(SqFull(back)) => {
                    // The SQ is ours and the fd is valid; enter failing here is unrecoverable.
                    self.ring
                        .submit_and_wait(0)
                        .expect("io_uring_enter failed while flushing a full SQ");
                    entry = back;
                }
            }
        }
    }
}
