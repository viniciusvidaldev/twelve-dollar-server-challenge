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
    uring::IoUring,
};

pub struct Server {
    listener: TcpListener,
    ring: IoUring,
    conns: Slab,
    app: App,
}

impl Server {
    pub fn new(ring: IoUring, listener: TcpListener) -> Self {
        Self {
            listener,
            ring,
            conns: Slab::new(),
            app: App::new(),
        }
    }

    pub fn run(&mut self) -> io::Result<()> {
        self.submit_accept()?;
        loop {
            self.ring.submit_and_wait(1)?;
            while let Some(cqe) = self.ring.next_cqe() {
                let token = Token::unpack(cqe.user_data);
                if cqe.res < 0 {
                    eprintln!("{}: {}", token, io::Error::from_raw_os_error(-cqe.res));
                }
                match token {
                    Token::Accept => self.on_accept(cqe.res)?,
                    Token::Recv(slot) => self.on_recv(slot, cqe.res)?,
                    Token::Send(slot) => self.on_send(slot, cqe.res)?,
                    Token::Close(slot) => self.on_close(slot),
                }
            }
        }
    }

    // TODO: pause accept on EMFILE/ENFILE until a slot frees
    pub fn on_accept(&mut self, res: i32) -> io::Result<()> {
        if res >= 0 {
            let slot = self.conns.insert(Conn::new(res));
            self.submit_recv(slot)?;
        }
        self.submit_accept()
    }

    pub fn on_recv(&mut self, slot: u32, res: i32) -> io::Result<()> {
        let next = if res <= 0 {
            Next::Close
        } else {
            self.conns
                .get_mut(slot)
                .after_recv(res as usize, &mut self.app)
        };
        self.submit_next(slot, next)
    }

    pub fn on_send(&mut self, slot: u32, res: i32) -> io::Result<()> {
        let next = if res < 0 {
            Next::Close
        } else {
            self.conns
                .get_mut(slot)
                .after_send(res as usize, &mut self.app)
        };
        self.submit_next(slot, next)
    }

    pub fn on_close(&mut self, slot: u32) {
        self.conns.remove(slot);
    }

    pub fn submit_next(&mut self, slot: u32, next: Next) -> io::Result<()> {
        match next {
            Next::Send => self.submit_send(slot),
            Next::Recv => self.submit_recv(slot),
            Next::Close => self.submit_close(slot),
        }
    }
}
