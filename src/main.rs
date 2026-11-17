use std::{ffi::CString, io, net::TcpListener};

use crate::{app::App, db::Db, server::Server, uring::IoUring};

mod app;
mod db;
mod http;
mod mmap;
mod server;
mod sys;
mod uring;

// TODO: raise fd limit on startup
fn main() -> io::Result<()> {
    let path = std::env::var("SQLITE_PATH").expect("SQLITE_PATH not set");
    let path = CString::new(path).expect("SQLITE_PATH contains a nul byte");

    let db = Db::open(&path)?;
    db.exec(c"PRAGMA journal_mode=WAL;")?;
    db.exec(c"PRAGMA synchronous=NORMAL;")?;

    let app = App::new(db);
    let listener = TcpListener::bind("0.0.0.0:8080")?;
    let ring = IoUring::new(4096)?;
    let mut server = Server::new(ring, listener, app);
    server.run()
}
