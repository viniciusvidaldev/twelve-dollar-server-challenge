use std::ffi::{c_char, c_int, c_void};

/// Opaque handle owned by SQLite; only ever used behind a pointer.
#[repr(C)]
pub struct sqlite3 {
    // Private and zero sized, so it can't be constructed or inspected from Rust.
    _private: [u8; 0],
}

pub const SQLITE_OK: c_int = 0;

pub const SQLITE_OPEN_READWRITE: c_int = 0x0000_0002;
pub const SQLITE_OPEN_NOMUTEX: c_int = 0x0000_8000;

pub type exec_callback =
    Option<unsafe extern "C" fn(*mut c_void, c_int, *mut *mut c_char, *mut *mut c_char) -> c_int>;

#[link(name = "sqlite3")]
unsafe extern "C" {
    pub fn sqlite3_open_v2(
        filename: *const c_char,
        db: *mut *mut sqlite3,
        flags: c_int,
        vfs: *const c_char,
    ) -> c_int;
    pub fn sqlite3_close_v2(db: *mut sqlite3) -> c_int;
    pub fn sqlite3_exec(
        db: *mut sqlite3,
        sql: *const c_char,
        callback: exec_callback,
        arg: *mut c_void,
        errmsg: *mut *mut c_char,
    ) -> c_int;
    pub fn sqlite3_errmsg(db: *mut sqlite3) -> *const c_char;
}
