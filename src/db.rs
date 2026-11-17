use std::{
    ffi::{CStr, c_int},
    fmt, io,
    ptr::{self, NonNull},
};

use crate::sys::sqlite;

#[derive(Debug)]
pub struct Error {
    pub code: c_int,
    pub msg: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sqlite error {}: {}", self.code, self.msg)
    }
}

impl std::error::Error for Error {}

impl From<Error> for io::Error {
    fn from(e: Error) -> Self {
        io::Error::other(e)
    }
}

pub struct Db {
    raw: NonNull<sqlite::sqlite3>,
}

impl Db {
    pub fn open(path: &CStr) -> Result<Self, Error> {
        let mut raw = ptr::null_mut();
        let flags = sqlite::SQLITE_OPEN_READWRITE | sqlite::SQLITE_OPEN_NOMUTEX;
        let code = unsafe { sqlite::sqlite3_open_v2(path.as_ptr(), &mut raw, flags, ptr::null()) };

        let Some(raw) = NonNull::new(raw) else {
            return Err(Error {
                code,
                msg: String::from("out of memory opening database"),
            });
        };

        let db = Self { raw };
        if code != sqlite::SQLITE_OK {
            return Err(db.error(code));
        }
        Ok(db)
    }

    pub fn exec(&self, sql: &CStr) -> Result<(), Error> {
        let code = unsafe {
            sqlite::sqlite3_exec(
                self.raw.as_ptr(),
                sql.as_ptr(),
                None,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };

        if code != sqlite::SQLITE_OK {
            return Err(self.error(code));
        }
        Ok(())
    }

    fn error(&self, code: c_int) -> Error {
        let msg = unsafe { CStr::from_ptr(sqlite::sqlite3_errmsg(self.raw.as_ptr())) };
        Error {
            code,
            msg: msg.to_string_lossy().into_owned(),
        }
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        unsafe { sqlite::sqlite3_close_v2(self.raw.as_ptr()) };
    }
}
