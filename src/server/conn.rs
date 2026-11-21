use std::mem;

use crate::{
    app::{App, Handled},
    uring::FixedFile,
};

// a client that never completes a request would otherwise grow partial forever
const MAX_PARTIAL: usize = 4096;

pub struct Conn {
    file: FixedFile,
    // tail of an incomplete request; empty and unallocated between requests
    partial: Vec<u8>,
    // the kernel reads this until the send completes, so never touch it while one is in flight
    out: Vec<u8>,
    sent: usize,
}

enum Drained {
    Consumed(usize),
    Close,
}

impl Conn {
    pub fn new(file: FixedFile) -> Self {
        Self {
            file,
            partial: Vec::new(),
            out: Vec::with_capacity(4096),
            sent: 0,
        }
    }

    pub fn file(&self) -> FixedFile {
        self.file
    }

    pub fn unsent(&self) -> &[u8] {
        &self.out[self.sent..]
    }

    pub fn after_recv(&mut self, data: &[u8], app: &mut App) -> Next {
        if self.partial.is_empty() {
            let drained = drain(data, &mut self.out, app);
            let Drained::Consumed(n) = drained else {
                return Next::Close;
            };
            self.partial.extend_from_slice(&data[n..]);
        } else {
            self.partial.extend_from_slice(data);
            let drained = drain(&self.partial, &mut self.out, app);
            let Drained::Consumed(n) = drained else {
                return Next::Close;
            };
            self.partial.drain(..n);
        }

        if self.partial.len() > MAX_PARTIAL {
            return Next::Close;
        }
        if self.partial.is_empty() {
            self.partial = Vec::new();
        }
        // send before recv, the client may be blocked waiting on response
        if !self.out.is_empty() {
            return Next::Send;
        }
        Next::Recv
    }

    pub fn after_send(&mut self, n: usize) -> Next {
        self.sent += n;
        if self.sent < self.out.len() {
            return Next::Send;
        }
        self.out.clear();
        self.sent = 0;
        Next::Recv
    }
}

fn drain(data: &[u8], out: &mut Vec<u8>, app: &mut App) -> Drained {
    let mut start = 0;
    // one recv can carry several pipelined requests
    loop {
        match app.handle(&data[start..], out) {
            Handled::Responded { consumed } => start += consumed,
            Handled::NeedMore => return Drained::Consumed(start),
            Handled::Close => return Drained::Close,
        }
    }
}

pub enum Next {
    Recv,
    Send,
    Close,
}

const NONE: u32 = u32::MAX;

enum Entry {
    Vacant(u32),
    Occupied(Conn),
}

pub struct Slab {
    entries: Vec<Entry>,
    curr_top: u32,
}

impl Slab {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            curr_top: NONE,
        }
    }

    pub fn insert(&mut self, conn: Conn) -> u32 {
        if self.curr_top == NONE {
            self.entries.push(Entry::Occupied(conn));
            return (self.entries.len() - 1) as u32;
        }

        let slot = self.curr_top;
        let prev = mem::replace(&mut self.entries[slot as usize], Entry::Occupied(conn));
        self.curr_top = match prev {
            Entry::Vacant(next) => next,
            Entry::Occupied(_) => unreachable!("free list points at an occupied slot"),
        };
        slot
    }

    pub fn get_mut(&mut self, slot: u32) -> &mut Conn {
        match &mut self.entries[slot as usize] {
            Entry::Occupied(conn) => conn,
            Entry::Vacant(_) => panic!("slot {slot} is empty"),
        }
    }

    pub fn remove(&mut self, slot: u32) {
        let entry = &mut self.entries[slot as usize];
        if let Entry::Vacant(_) = entry {
            panic!("slot {slot} removed twice");
        }
        *entry = Entry::Vacant(self.curr_top);
        self.curr_top = slot;
    }
}
