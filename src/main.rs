use std::{io, net::TcpListener};

use crate::{server::Server, uring::IoUring};

mod app;
mod http;
mod mmap;
mod server;
mod sys;
mod uring;

// TODO: raise fd limit on startup

fn main() -> io::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:8080")?;
    let ring = IoUring::new(4096)?;
    let mut server = Server::new(ring, listener);
    server.run()
}
