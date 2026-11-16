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
        self.submit_accept();
        loop {
            self.ring.submit_and_wait(1)?;
            while let Some(cqe) = self.ring.next_cqe() {
                let res = cqe_result(cqe.res);
                match Token::unpack(cqe.user_data) {
                    Token::Accept => self.on_accept(res),
                    Token::Recv(slot) => self.on_recv(slot, res),
                    Token::Send(slot) => self.on_send(slot, res),
                    Token::Close(slot) => self.on_close(slot),
                }
            }
        }
    }

    // TODO: pause accept on EMFILE/ENFILE until a slot frees
    fn on_accept(&mut self, res: io::Result<usize>) {
        match res {
            Ok(fd) => {
                let slot = self.conns.insert(Conn::new(fd as i32));
                self.submit_recv(slot);
            }
            Err(e) => eprintln!("accept: {e}"),
        }
        self.submit_accept();
    }

    fn on_recv(&mut self, slot: u32, res: io::Result<usize>) {
        let next = self.conns.get_mut(slot).after_recv(res, &mut self.app);
        self.submit_next(slot, next);
    }

    fn on_send(&mut self, slot: u32, res: io::Result<usize>) {
        let next = self.conns.get_mut(slot).after_send(res, &mut self.app);
        self.submit_next(slot, next);
    }

    fn on_close(&mut self, slot: u32) {
        self.conns.remove(slot);
    }

    fn submit_next(&mut self, slot: u32, next: Next) {
        match next {
            Next::Send => self.submit_send(slot),
            Next::Recv => self.submit_recv(slot),
            Next::Close => self.submit_close(slot),
        }
    }
}

fn cqe_result(res: i32) -> io::Result<usize> {
    if res < 0 {
        Err(io::Error::from_raw_os_error(-res))
    } else {
        Ok(res as usize)
    }
}
