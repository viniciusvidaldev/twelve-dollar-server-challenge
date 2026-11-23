mod conn;
mod submit;
mod token;

use std::{
    io,
    net::TcpListener,
    os::fd::{AsRawFd, OwnedFd, RawFd},
};

use crate::{
    app::App,
    server::{
        conn::{Conn, Next, Slab},
        token::Token,
    },
    sys::io_uring,
    uring::{BufferId, FixedFile, IoUring},
};

struct Shard {
    ring: IoUring,
    conns: Slab,
    accept_paused: bool,
}

pub struct Server {
    listener: TcpListener,
    shards: Vec<Shard>,
    // list of eventfd's
    wakers: Vec<OwnedFd>,
    app: App,
}

impl Server {
    pub fn new(rings: Vec<IoUring>, wakers: Vec<OwnedFd>, listener: TcpListener, app: App) -> Self {
        // run() polls wakers[i] from ring 0 to wake for rings[i + 1]
        assert_eq!(
            wakers.len(),
            rings.len() - 1,
            "one waker per ring except ring 0"
        );

        Self {
            listener,
            shards: rings.into_iter().map(Shard::new).collect(),
            wakers,
            app,
        }
    }

    pub fn run(&mut self) -> io::Result<()> {
        let listen_fd = self.listener.as_raw_fd();
        for shard in &mut self.shards {
            shard.submit_accept(listen_fd);
        }

        // only shard 0 blocks; the others wake it through these polls
        for waker in &self.wakers {
            self.shards[0].submit_wake(waker.as_raw_fd());
        }

        loop {
            // drain first; handling completions queues new sqes
            for shard in &mut self.shards {
                shard.drain(listen_fd, &mut self.app);
            }

            // then submit everything queued, before sleeping
            for shard in &mut self.shards[1..] {
                // an overflowed cq needs an enter to flush, even with nothing to submit
                if shard.ring.has_pending() || shard.ring.cq_overflowed() {
                    shard.ring.submit_and_wait(0)?;
                }
            }

            // any ring's cqe lands here, directly or as a wake
            self.shards[0].ring.submit_and_wait(1)?;
        }
    }
}

impl Shard {
    fn new(ring: IoUring) -> Self {
        Self {
            ring,
            conns: Slab::new(),
            accept_paused: false,
        }
    }

    fn drain(&mut self, listen_fd: RawFd, app: &mut App) {
        while let Some(cqe) = self.ring.next_cqe() {
            let c = Completion::new(cqe);
            match c.token() {
                Token::Accept => self.on_accept(c, listen_fd),
                Token::Recv(slot) => self.on_recv(slot, c, app),
                Token::Send(slot) => self.on_send(slot, c),
                Token::Close(slot) => self.on_close(slot, listen_fd),
                Token::Wake(fd) => self.on_wake(c, fd),
            }
        }
    }

    fn on_accept(&mut self, c: Completion, listen_fd: RawFd) {
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
        if !c.armed() {
            self.submit_accept(listen_fd);
        }
    }

    fn on_recv(&mut self, slot: u32, c: Completion, app: &mut App) {
        let next = match c.result() {
            Ok(0) => Next::Close,
            Ok(n) => 'recv: {
                let Some(bid) = c.buffer_id() else {
                    break 'recv Next::Close;
                };

                let data = self.ring.bufs().get(&bid, n);
                let next = self.conns.get_mut(slot).after_recv(data, app);
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

    fn on_close(&mut self, slot: u32, listen_fd: RawFd) {
        self.conns.remove(slot);
        if self.accept_paused {
            self.accept_paused = false;
            self.submit_accept(listen_fd);
        }
    }

    fn on_wake(&mut self, c: Completion, fd: RawFd) {
        if !c.armed() {
            self.submit_wake(fd);
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

    fn armed(&self) -> bool {
        self.0.flags & io_uring::IORING_CQE_F_MORE != 0
    }

    fn token(&self) -> Token {
        Token::unpack(self.0.user_data)
    }

    fn buffer_id(&self) -> Option<BufferId> {
        BufferId::from_cqe_flags(self.0.flags)
    }
}
