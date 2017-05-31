pub mod state;

use std::ffi::{CStr, CString};
use std::io::{self, Read, Write};
#[cfg(unix)]
use std::os::unix::io::RawFd;
use std::net::TcpStream;
use std::ptr;
use std::str;

use glib_sys;

use error;
use Source;
use super::Inner;
use self::state::IoChannelFuncs;

/// Wrapper around the underlying glib `GIOChannel` type
pub struct IoChannel {
    inner: *mut glib_sys::GIOChannel,
}

unsafe impl Send for IoChannel {} // FIXME: probably wrong

impl IoChannel {
    #[cfg(unix)]
    pub fn unix_new(fd: RawFd) -> Self {
        let ptr = unsafe {
            glib_sys::g_io_channel_unix_new(fd)
        };
        assert!(!ptr.is_null());
        IoChannel {
            inner: ptr,
        }
    }

    /// Gets the internal buffer size.
    pub fn buffer_size(&self) -> usize {
        unsafe {
            glib_sys::g_io_channel_get_buffer_size(self.inner)
        }
    }

    /// Sets the buffer size.
    ///
    /// If 0, then glib will pick a good size.
    pub fn set_buffer_size(&self, size: usize) {
        unsafe {
            glib_sys::g_io_channel_set_buffer_size(self.inner, size)
        }
    }

    /// Returns whether this channel is buffered.
    pub fn is_buffered(&self) -> bool {
        unsafe {
            glib_sys::g_io_channel_get_buffered(self.inner) != 0
        }
    }

    /// Sets whether this channel is buffered or not.
    ///
    /// The buffering state can only be set if the channel's encoding is null.
    /// For any other encoding, the channel must be buffered.
    ///
    /// A buffered channel can only be set unbuffered if the channel's internal
    /// buffers have been flushed. Newly created channels or channels which
    /// have returned `G_IO_STATUS_EOF` don't require such a flush. For
    /// write-only channels, a call to `flush is sufficient. Note that this
    /// means that socket-based channels cannot be set unbuffered once they
    /// have had data read from them.
    ///
    /// The default state of the channel is buffered.
    pub fn set_buffered(&self, buffered: bool) {
        unsafe {
            glib_sys::g_io_channel_set_buffered(self.inner, buffered as i32)
        }
    }

    /// Returns whether the file/socket/whatever associated with channel will
    /// be closed when channel receives its final unref and is destroyed.
    ///
    /// The default value of this is true for channels created by
    /// `g_io_channel_new_file()`, and false for all other channels.
    pub fn is_close_on_drop(&self) -> bool {
        unsafe {
            glib_sys::g_io_channel_get_close_on_unref(self.inner) != 0
        }
    }

    /// Sets whether this channel will close the underlying I/O object whent he
    /// last reference goes out of scope.
    ///
    /// Setting this flag to true for a channel you have already closed can
    /// cause problems.
    pub fn set_close_on_drop(&self, close: bool) {
        unsafe {
            glib_sys::g_io_channel_set_close_on_unref(self.inner, close as i32)
        }
    }

    /// Close an IO channel.
    ///
    /// Any pending data to be written will be flushed if flush is `true`. The
    /// channel will not be freed until the last reference is dropped.
    pub fn close(&mut self, flush: bool) -> io::Result<()> {
        unsafe {
            let mut error = 0 as *mut _;
            let r = glib_sys::g_io_channel_shutdown(self.inner,
                                                   flush as i32,
                                                   &mut error);
            rc(r, None, error).map(|_| ())
        }
    }

    /// Creates a `Source` that's dispatched when condition is met for the given
    /// channel. For example, if condition is `G_IO_IN`, the source will be
    /// dispatched when there's data available for reading.
    ///
    /// On Windows, polling a `Source` created to watch a channel for a socket
    /// puts the socket in non-blocking mode. This is a side-effect of the
    /// implementation and unavoidable.
    pub fn create_watch(&self, condition: &IoCondition) -> Source<IoChannelFuncs> {
        unsafe {
            let ptr = glib_sys::g_io_create_watch(self.inner, condition.bits);
            assert!(!ptr.is_null());
            ptr::write(&mut (*(ptr as *mut Inner<IoChannelFuncs>)).data, IoChannelFuncs::new(self.clone()));
            ::source_new(ptr)
        }
    }

    /// Gets the encoding for the input/output of the channel.
    ///
    /// The internal encoding is always UTF-8. The encoding `None` makes the
    /// channel safe for binary data.
    pub fn encoding(&self) -> Option<&str> {
        unsafe {
            let ptr = glib_sys::g_io_channel_get_encoding(self.inner);
            if ptr.is_null() {
                None
            } else {
                Some(str::from_utf8(CStr::from_ptr(ptr).to_bytes()).unwrap())
            }
        }
    }

    /// Sets the encoding for the input/output of the channel. The internal
    /// encoding is always UTF-8. The default encoding for the external file is
    /// UTF-8.
    ///
    /// The encoding `None` is safe to use with binary data.
    pub fn set_encoding(&self, encoding: Option<&str>) -> io::Result<()> {
        unsafe {
            let mut error = 0 as *mut _;
            let encoding = match encoding {
                Some(s) => Some(CString::new(s)?),
                None => None,
            };
            let encoding = encoding.as_ref().map(|c| c.as_ptr()).unwrap_or(0 as *const _);
            let r = glib_sys::g_io_channel_set_encoding(self.inner,
                                                        encoding,
                                                        &mut error);
            rc(r, None, error).map(|_| ())
        }
    }
}

unsafe fn rc(rc: glib_sys::GIOStatus,
             amt: Option<usize>,
             error: *mut glib_sys::GError) -> Result<usize, io::Error> {
    match rc {
        glib_sys::G_IO_STATUS_ERROR => {
            if let Some(amt) = amt {
                assert_eq!(amt, 0);
            }
            assert!(!error.is_null());
            Err(error::new(error).into())
        }
        glib_sys::G_IO_STATUS_NORMAL => {
            assert!(error.is_null());
            Ok(amt.unwrap_or(0))
        }
        glib_sys::G_IO_STATUS_EOF => {
            if let Some(amt) = amt {
                assert_eq!(amt, 0);
            }
            assert!(error.is_null());
            Ok(0)
        }
        glib_sys::G_IO_STATUS_AGAIN => {
            if let Some(amt) = amt {
                assert_eq!(amt, 0);
            }
            assert!(error.is_null());
            Err(io::Error::from(io::ErrorKind::WouldBlock))
        }
    }
}

impl Read for IoChannel {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        <&IoChannel>::read(&mut &*self, buf)
    }
}

impl<'a> Read for &'a IoChannel {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        unsafe {
            let mut read = 0;
            let mut error = 0 as *mut _;
            let r = glib_sys::g_io_channel_read_chars(self.inner,
                                                      buf.as_mut_ptr() as *mut _,
                                                      buf.len(),
                                                      &mut read,
                                                      &mut error);
            rc(r, Some(read), error)
        }
    }
}

impl Write for IoChannel {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        <&IoChannel>::write(&mut &*self, buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        <&IoChannel>::flush(&mut &*self)
    }
}

impl<'a> Write for &'a IoChannel {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        unsafe {
            let mut written = 0;
            let mut error = 0 as *mut _;
            let r = glib_sys::g_io_channel_write_chars(self.inner,
                                                       buf.as_ptr() as *mut _,
                                                       buf.len() as isize, // TODO: checked cast
                                                       &mut written,
                                                       &mut error);
            rc(r, Some(written), error)
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        unsafe {
            let mut error = 0 as *mut _;
            let r = glib_sys::g_io_channel_flush(self.inner, &mut error);
            rc(r, None, error).map(|_| ())
        }
    }
}

impl From<TcpStream> for IoChannel {
    #[cfg(unix)]
    fn from(socket: TcpStream) -> IoChannel {
        use std::os::unix::prelude::*;

        let ptr = unsafe {
            glib_sys::g_io_channel_unix_new(socket.into_raw_fd())
        };
        assert!(!ptr.is_null());
        IoChannel {
            inner: ptr,
        }
    }

    #[cfg(windows)]
    fn from(socket: TcpStream) -> IoChannel {
        use std::os::windows::prelude::*;

        let ptr = unsafe {
            g_io_channel_win32_new_socket(socket.into_raw_socket() as i32)
        };
        assert!(!ptr.is_null());
        IoChannel {
            inner: ptr,
        }
    }
}

#[cfg(windows)]
extern "C" {
    fn g_io_channel_win32_new_socket(socket: ::libc::c_int) -> *mut glib_sys::GIOChannel;
}

impl Clone for IoChannel {
    fn clone(&self) -> IoChannel {
        unsafe {
            IoChannel {
                inner: glib_sys::g_io_channel_ref(self.inner),
            }
        }
    }
}

impl Drop for IoChannel {
    fn drop(&mut self) {
        unsafe {
            glib_sys::g_io_channel_unref(self.inner);
        }
    }
}

/// A bitwise combination representing a condition to watch for on an event
/// source.
#[derive(Debug, Clone)]
pub struct IoCondition {
    bits: glib_sys::GIOCondition,
}

pub fn bits(condition: &IoCondition) -> glib_sys::GIOCondition {
    condition.bits
}

pub fn bits_new(bits: glib_sys::GIOCondition) -> IoCondition {
    IoCondition { bits: bits }
}

impl IoCondition {
    /// Creates a new bit set with no interest bits set.
    pub fn new() -> IoCondition {
        IoCondition {
            bits: glib_sys::GIOCondition::empty(),
        }
    }

    /// Flag indicating that data is ready to read.
    pub fn input(&mut self, input: bool) -> &mut IoCondition {
        self.flag(input, glib_sys::G_IO_IN)
    }

    /// Flag indicating that data can be written.
    pub fn output(&mut self, output: bool) -> &mut IoCondition {
        self.flag(output, glib_sys::G_IO_OUT)
    }

    /// Tests whether this condition indicates input readiness
    pub fn is_input(&self) -> bool {
        self.bits.contains(glib_sys::G_IO_IN)
    }

    /// Tests whether this condition indicates output readiness
    pub fn is_output(&self) -> bool {
        self.bits.contains(glib_sys::G_IO_OUT)
    }

    fn flag(&mut self, enabled: bool, flag: glib_sys::GIOCondition) -> &mut IoCondition {
        if enabled {
            self.bits = self.bits | flag;
        } else {
            self.bits = self.bits & !flag;
        }
        self
    }
}

#[cfg(unix)]
mod unix {
    use std::os::unix::prelude::*;

    use glib_sys;

    use IoChannel;

    impl AsRawFd for IoChannel {
        fn as_raw_fd(&self) -> RawFd {
            unsafe {
                glib_sys::g_io_channel_unix_get_fd(self.inner)
            }
        }
    }
}
