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
    uring::{BufferId, FixedFile, IoUring},
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
                let c = Completion::new(cqe);
                match c.token() {
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
            Ok(res) => {
                let file = FixedFile::from_accept(res as u32);
                let slot = self.conns.insert(Conn::new(file));
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
        let next = match c.result() {
            Ok(0) => Next::Close,
            Ok(n) => 'recv: {
                let Some(bid) = c.buffer_id() else {
                    break 'recv Next::Close;
                };

                let data = self.ring.bufs().get(&bid, n);
                let next = self.conns.get_mut(slot).after_recv(data, &mut self.app);
                self.ring.bufs_mut().recycle(bid);
                next
            }
            Err(e) if e.raw_os_error() == Some(io_uring::ENOBUFS) => Next::Recv,
            Err(_) => Next::Close,
        };
        self.submit_next(slot, next);
    }

    fn on_send(&mut self, slot: u32, c: Completion) {
        let next = match c.result() {
            Ok(0) | Err(_) => Next::Close,
            Ok(n) => self.conns.get_mut(slot).after_send(n),
        };
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
struct Completion(io_uring::io_uring_cqe);

impl Completion {
    fn new(cqe: io_uring::io_uring_cqe) -> Self {
        Self(cqe)
    }

    fn result(&self) -> io::Result<usize> {
        if self.0.res < 0 {
            return Err(io::Error::from_raw_os_error(-self.0.res));
        }
        Ok(self.0.res as usize)
    }

    fn more(&self) -> bool {
        self.0.flags & io_uring::IORING_CQE_F_MORE != 0
    }

    fn token(&self) -> Token {
        Token::unpack(self.0.user_data)
    }

    fn buffer_id(&self) -> Option<BufferId> {
        BufferId::from_cqe_flags(self.0.flags)
    }
}
