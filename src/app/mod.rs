mod ids;

pub use ids::{PostId, UserId};

use crate::{
    db::Db,
    http::{self, Method, Parse},
};

pub enum Handled {
    NeedMore,
    Responded { consumed: usize },
    Close,
}

pub struct App {
    db: Db,
}

impl App {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub fn handle(&mut self, input: &[u8], out: &mut Vec<u8>) -> Handled {
        let req = match http::parse(&input) {
            Parse::Invalid => return Handled::Close,
            Parse::Partial => return Handled::NeedMore,
            Parse::Done(req) => req,
        };

        match (req.method, req.path) {
            (Method::Get, b"/") => respond(out, 200, b"ok\n"),
            _ => respond(out, 404, b""),
        }

        Handled::Responded { consumed: req.len }
    }
}

fn respond(out: &mut Vec<u8>, status: u16, body: &[u8]) {
    use std::io::Write;
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        _ => "",
    };
    let _ = write!(
        out,
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    out.extend_from_slice(body);
}
