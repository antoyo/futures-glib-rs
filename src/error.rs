use std::error;
use std::ffi::CStr;
use std::fmt;
use std::io;
use std::str;

use glib_sys;

pub struct Error {
    inner: *mut glib_sys::GError,
}

unsafe impl Send for Error {} // TODO: check this is true
unsafe impl Sync for Error {} // TODO: check this is true

pub unsafe fn new(inner: *mut glib_sys::GError) -> Error {
    Error { inner: inner }
}

impl Error {
    /// Domain of this error, e.g. G_FILE_ERROR
    pub fn domain(&self) -> u32 {
        unsafe { (*self.inner).domain }
    }

    /// Error code for this error e.g. G_FILE_ERROR_NOENT
    pub fn code(&self) -> i32 {
        unsafe { (*self.inner).code }
    }

    /// Returns the human-readable informative error message
    ///
    /// # Panics
    ///
    /// If the message is not valid utf-8, then this method will panic
    pub fn message(&self) -> &str {
        str::from_utf8(self.message_bytes()).unwrap()
    }

    /// Returns the human-readable informative error message as a list of bytes
    pub fn message_bytes(&self) -> &[u8] {
        unsafe {
            CStr::from_ptr((*self.inner).message).to_bytes()
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "[{}/{}] {}", self.domain(), self.code(), self.message())
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Error")
         .field("domain", &self.domain())
         .field("code", &self.code())
         .field("message", &self.message())
         .finish()
    }
}

impl error::Error for Error {
    fn description(&self) -> &str {
        self.message()
    }
}

impl Drop for Error {
    fn drop(&mut self) {
        unsafe {
            glib_sys::g_error_free(self.inner);
        }
    }
}

impl From<Error> for io::Error {
    fn from(e: Error) -> io::Error {
        // TODO: needs a better error kind here
        io::Error::new(io::ErrorKind::Other, e)
    }
}
