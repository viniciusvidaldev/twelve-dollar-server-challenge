use std::{ffi::CString, io, net::TcpListener};

use crate::{
    app::{App, Auth},
    db::Db,
    server::Server,
    uring::IoUring,
};

mod app;
mod db;
mod http;
mod mmap;
mod rlimit;
mod server;
mod sys;
mod uring;

fn main() -> io::Result<()> {
    let path = std::env::var("SQLITE_PATH").expect("SQLITE_PATH not set");
    let path = CString::new(path).expect("SQLITE_PATH contains a nul byte");

    let db = Db::open(&path)?;

    let secret = std::env::var("JWT_SECRET").expect("JWT_SECRET not set");
    let auth = Auth::new(secret.as_bytes()).map_err(io::Error::other)?;

    let app = App::new(db, auth);

    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into());
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".into());
    let listener = TcpListener::bind(format!("{host}:{port}"))?;

    // Raises soft fd limit.
    let nofile = rlimit::raise_nofile()?;
    let ring = IoUring::new(4096)?;
    ring.register_files_sparse(nofile.min(65535) as u32)?;

    let mut server = Server::new(ring, listener, app);
    server.run()
}
