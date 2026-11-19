mod conn;
mod submit;
mod token;

use std::{io, net::TcpListener};

use crate::{
    app::App,
    server::{
        conn::{Conn, Next, Slab},
        token::Token,
    },
    sys::io_uring,
    uring::IoUring,
};

pub struct Server {
    listener: TcpListener,
    ring: IoUring,
    conns: Slab,
    app: App,
    accept_paused: bool,
}

impl Server {
    pub fn new(ring: IoUring, listener: TcpListener, app: App) -> Self {
        Self {
            listener,
            ring,
            conns: Slab::new(),
            app,
            accept_paused: false,
        }
    }

    pub fn run(&mut self) -> io::Result<()> {
        self.submit_accept();
        loop {
            self.ring.submit_and_wait(1)?;
            while let Some(cqe) = self.ring.next_cqe() {
                let c = Completion::new(&cqe);
                match Token::unpack(cqe.user_data) {
                    Token::Accept => self.on_accept(c),
                    Token::Recv(slot) => self.on_recv(slot, c),
                    Token::Send(slot) => self.on_send(slot, c),
                    Token::Close(slot) => self.on_close(slot, c),
                }
            }
        }
    }

    fn on_accept(&mut self, c: Completion) {
        const ENFILE: i32 = 23;
        const EMFILE: i32 = 24;

        match c.result() {
            Ok(fd) => {
                let slot = self.conns.insert(Conn::new(fd as i32));
                self.submit_recv(slot);
            }
            Err(e) if matches!(e.raw_os_error(), Some(EMFILE | ENFILE)) => {
                // resumed by on_close once a connection frees an fd
                self.accept_paused = true;
                return;
            }
            Err(e) => eprintln!("accept: {e}"),
        }
        if !c.more() {
            self.submit_accept();
        }
    }

    fn on_recv(&mut self, slot: u32, c: Completion) {
        let next = self
            .conns
            .get_mut(slot)
            .after_recv(c.result(), &mut self.app);
        self.submit_next(slot, next);
    }

    fn on_send(&mut self, slot: u32, c: Completion) {
        let next = self
            .conns
            .get_mut(slot)
            .after_send(c.result(), &mut self.app);
        self.submit_next(slot, next);
    }

    fn on_close(&mut self, slot: u32, _c: Completion) {
        self.conns.remove(slot);
        if self.accept_paused {
            self.accept_paused = false;
            self.submit_accept();
        }
    }

    fn submit_next(&mut self, slot: u32, next: Next) {
        match next {
            Next::Send => self.submit_send(slot),
            Next::Recv => self.submit_recv(slot),
            Next::Close => self.submit_close(slot),
        }
    }
}

#[derive(Clone, Copy)]
struct Completion {
    res: i32,
    flags: u32,
}

impl Completion {
    fn new(cqe: &io_uring::io_uring_cqe) -> Self {
        Self {
            res: cqe.res,
            flags: cqe.flags,
        }
    }

    fn result(&self) -> io::Result<usize> {
        if self.res < 0 {
            return Err(io::Error::from_raw_os_error(-self.res));
        }
        Ok(self.res as usize)
    }

    fn more(&self) -> bool {
        self.flags & io_uring::IORING_CQE_F_MORE != 0
    }
}
