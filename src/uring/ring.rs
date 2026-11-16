use std::{
    io,
    os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd},
    ptr,
    sync::atomic::{AtomicU32, Ordering},
};

use super::op::{Entry, Tagged};
use crate::mmap::Mmap;
use crate::sys::{cvt, io_uring};

pub struct IoUring {
    fd: OwnedFd,
    _ring_map: Mmap,
    _sqe_map: Mmap,
    sq: SubmissionQueue,
    cq: CompletionQueue,
}

pub struct SqFull(pub Entry<Tagged>);

impl IoUring {
    pub fn new(entries: u32) -> io::Result<Self> {
        let mut p = io_uring::io_uring_params::default();
        p.flags = io_uring::IORING_SETUP_NO_SQARRAY;
        let fd = cvt(unsafe { io_uring::io_uring_setup(entries, &mut p) })?;
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };

        if p.features & io_uring::IORING_FEAT_SINGLE_MMAP == 0 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "kernel lacks IORING_FEAT_SINGLE_MMAP",
            ));
        }

        let sq_len = p.sq_off.array as usize + p.sq_entries as usize * size_of::<u32>();
        let cq_len =
            p.cq_off.cqes as usize + p.cq_entries as usize * size_of::<io_uring::io_uring_cqe>();
        let ring_len = sq_len.max(cq_len);
        let ring_map = Mmap::new(fd.as_fd(), ring_len, io_uring::IORING_OFF_SQ_RING)?;

        let sqe_len = p.sq_entries as usize * size_of::<io_uring::io_uring_sqe>();
        let sqe_map = Mmap::new(fd.as_fd(), sqe_len, io_uring::IORING_OFF_SQES)?;

        let cq = unsafe {
            let base = ring_map.as_ptr();
            let off = &p.cq_off;
            CompletionQueue {
                head: base.add(off.head as usize).cast(),
                tail: base.add(off.tail as usize).cast(),
                mask: *base.add(off.ring_mask as usize).cast::<u32>(),
                entries: *base.add(off.ring_entries as usize).cast::<u32>(),
                cqes: base.add(off.cqes as usize).cast(),
                local_head: (*base.add(off.head as usize).cast::<AtomicU32>())
                    .load(Ordering::Relaxed),
                cached_tail: (*base.add(off.tail as usize).cast::<AtomicU32>())
                    .load(Ordering::Acquire),
            }
        };

        let sq = unsafe {
            let base = ring_map.as_ptr();
            let off = &p.sq_off;
            SubmissionQueue {
                head: base.add(off.head as usize).cast(),
                tail: base.add(off.tail as usize).cast(),
                flags: base.add(off.flags as usize).cast(),
                mask: *base.add(off.ring_mask as usize).cast::<u32>(),
                entries: *base.add(off.ring_entries as usize).cast::<u32>(),
                sqes: sqe_map.as_ptr().cast(),
                local_tail: (*base.add(off.tail as usize).cast::<AtomicU32>())
                    .load(Ordering::Relaxed),
                cached_head: (*base.add(off.head as usize).cast::<AtomicU32>())
                    .load(Ordering::Acquire),
            }
        };

        let s = Self {
            fd,
            _ring_map: ring_map,
            _sqe_map: sqe_map,
            sq,
            cq,
        };
        Ok(s)
    }

    /// Refreshes `cached_head` only when the cached view looks full.
    fn sq_has_room(&mut self) -> bool {
        if self.sq.local_tail.wrapping_sub(self.sq.cached_head) < self.sq.entries {
            return true;
        }
        // Acquire pairs with the kernel's head store.
        self.sq.cached_head = unsafe { (*self.sq.head).load(Ordering::Acquire) };
        self.sq.local_tail.wrapping_sub(self.sq.cached_head) < self.sq.entries
    }

    /// # Safety
    /// Any buffer referenced by `entry` must stay valid until its CQE is acquired.
    pub unsafe fn push(&mut self, entry: Entry<Tagged>) -> Result<(), SqFull> {
        let tail = self.sq.local_tail;
        if !self.sq_has_room() {
            return Err(SqFull(entry));
        }

        let idx = tail & self.sq.mask;
        unsafe {
            let slot = self.sq.sqes.add(idx as usize);
            slot.write(entry.sqe);
        }
        self.sq.local_tail = self.sq.local_tail.wrapping_add(1);
        Ok(())
    }

    fn flush_sq(&mut self) -> u32 {
        // Release makes every SQE written by push visible before the kernel sees the new tail.
        unsafe { (*self.sq.tail).store(self.sq.local_tail, Ordering::Release) };
        self.sq.cached_head = unsafe { (*self.sq.head).load(Ordering::Acquire) };
        self.sq.local_tail.wrapping_sub(self.sq.cached_head)
    }

    pub fn submit_and_wait(&mut self, want: u32) -> io::Result<u32> {
        let to_submit = self.flush_sq();
        self.sync_cq();
        let flags = if want > 0 {
            io_uring::IORING_ENTER_GETEVENTS
        } else {
            0
        };
        let fd = self.fd.as_raw_fd();
        loop {
            let ret =
                unsafe { io_uring::io_uring_enter(fd, to_submit, want, flags, ptr::null(), 0) };
            match cvt(ret) {
                Ok(n) => return Ok(n as u32),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
    }

    /// Refreshes `cached_tail` only when the cached view looks empty.
    fn cq_has_ready(&mut self) -> bool {
        if self.cq.cached_tail != self.cq.local_head {
            return true;
        }
        // Acquire pairs with the kernel's tail store.
        self.cq.cached_tail = unsafe { (*self.cq.tail).load(Ordering::Acquire) };
        self.cq.cached_tail != self.cq.local_head
    }

    fn sync_cq(&mut self) {
        // Release tells the kernel the reaped slots are free.
        unsafe { (*self.cq.head).store(self.cq.local_head, Ordering::Release) };
    }

    pub fn next_cqe(&mut self) -> Option<io_uring::io_uring_cqe> {
        if !self.cq_has_ready() {
            self.sync_cq();
            return None;
        }
        let idx = self.cq.local_head & self.cq.mask;
        let cqe = unsafe { self.cq.cqes.add(idx as usize).read() };
        self.cq.local_head = self.cq.local_head.wrapping_add(1);
        Some(cqe)
    }
}

struct SubmissionQueue {
    head: *const AtomicU32,
    tail: *const AtomicU32,
    flags: *const AtomicU32,
    mask: u32,
    entries: u32,
    sqes: *mut io_uring::io_uring_sqe,
    local_tail: u32,
    cached_head: u32,
}

struct CompletionQueue {
    head: *const AtomicU32,
    tail: *const AtomicU32,
    mask: u32,
    entries: u32,
    cqes: *const io_uring::io_uring_cqe,
    local_head: u32,
    cached_tail: u32,
}
