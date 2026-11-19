use std::{io, mem};

use crate::{
    app::{App, Handled},
    uring::FixedFile,
};

pub struct Conn {
    file: FixedFile,
    // Fixed size so its address stays put while a recv is in flight.
    inbuf: Box<[u8]>,
    filled: usize,
    // Only mutated while no send is in flight; the kernel reads it until the send completes.
    out: Vec<u8>,
    sent: usize,
}

impl Conn {
    pub fn new(file: FixedFile) -> Self {
        Self {
            file,
            inbuf: vec![0u8; 4096].into_boxed_slice(),
            filled: 0,
            out: Vec::with_capacity(4096),
            sent: 0,
        }
    }

    pub fn file(&self) -> FixedFile {
        self.file
    }

    pub fn recv_buf(&mut self) -> &mut [u8] {
        &mut self.inbuf[self.filled..]
    }

    pub fn unsent(&self) -> &[u8] {
        &self.out[self.sent..]
    }

    pub fn after_recv(&mut self, res: io::Result<usize>, app: &mut App) -> Next {
        let n = match res {
            Ok(0) | Err(_) => return Next::Close,
            Ok(n) => n,
        };
        self.filled += n;
        self.process(app)
    }

    pub fn after_send(&mut self, res: io::Result<usize>, app: &mut App) -> Next {
        let Ok(n) = res else {
            return Next::Close;
        };
        self.sent += n;
        if self.sent < self.out.len() {
            return Next::Send;
        }
        self.out.clear();
        self.sent = 0;
        self.process(app)
    }

    fn process(&mut self, app: &mut App) -> Next {
        let mut start = 0;
        loop {
            match app.handle(&self.inbuf[start..self.filled], &mut self.out) {
                Handled::Responded { consumed } => start += consumed,
                Handled::NeedMore => break,
                Handled::Close => return Next::Close,
            }
        }
        self.inbuf.copy_within(start..self.filled, 0);
        self.filled -= start;
        if !self.out.is_empty() {
            return Next::Send;
        }
        // Only valid for this API: all its requests fit in inbuf.
        if self.filled == self.inbuf.len() {
            return Next::Close;
        }
        Next::Recv
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
