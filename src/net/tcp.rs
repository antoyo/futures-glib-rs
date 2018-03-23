use std::cell::RefCell;
use std::io::{self, Read, Write};
use std::net::SocketAddr;
#[cfg(unix)]
use std::os::unix::prelude::*;
use std::time::Duration;

use bytes::{Buf, BufMut};
use futures::task;
#[cfg(unix)]
use libc::EINPROGRESS;
#[cfg(windows)]
use winapi::shared::winerror::WSAEINPROGRESS as EINPROGRESS;

use futures::{Future, Poll, Async};
use glib_sys;
use net2::{TcpBuilder, TcpStreamExt};
use tokio_io::{AsyncRead, AsyncWrite};

use {Source, SourceFuncs, IoChannel, IoCondition, MainContext};
use io::state::State;
#[cfg(unix)]
use UnixToken;
#[cfg(windows)]
use io::IoChannelFuncs;

#[cfg(unix)]
fn create_source(channel: IoChannel, active: IoCondition, context: &MainContext) -> Source<Inner> {
    let fd = channel.as_raw_fd();
    // Wrap the channel itself in a `Source` that we create and manage.
    let src = Source::new(Inner {
        channel: channel,
        active: RefCell::new(active.clone()),
        read: RefCell::new(State::NotReady),
        write: RefCell::new(State::NotReady),
        token: RefCell::new(None),
    });
    src.attach(context);

    // And finally, add the file descriptor to the source so it knows
    // what we're tracking.
    let t = src.unix_add_fd(fd, &active);
    *src.get_ref().token.borrow_mut() = Some(t);
    src
}

#[cfg(windows)]
fn create_source(channel: IoChannel, active: IoCondition, _context: &MainContext) -> Source<IoChannelFuncs> {
    channel.create_watch(&active)
}

/// A raw TCP byte stream connected to a remote address.
pub struct TcpStream {
    #[cfg(unix)]
    inner: Source<Inner>,
    #[cfg(windows)]
    inner: Source<IoChannelFuncs>,
}

#[cfg(unix)]
struct Inner {
    channel: IoChannel,
    active: RefCell<IoCondition>,
    token: RefCell<Option<UnixToken>>,
    read: RefCell<State>,
    write: RefCell<State>,
}

/// Future returned from `TcpStream::connect` representing a connecting TCP
/// stream.
pub struct TcpStreamConnect {
    state: Option<io::Result<TcpStream>>,
}

impl TcpStream {
    /// Creates a new TCP stream that will connect to the specified address.
    ///
    /// The returned TCP stream will be associated with the provided context. A
    /// future is returned representing the connected TCP stream.
    pub fn connect(addr: &SocketAddr, context: &MainContext) -> TcpStreamConnect {
        let socket = (|| -> io::Result<_> {
            // Use net2 to create the raw socket, set it to nonblocking mode,
            // and then issue a connection to the remote address
            let socket = match *addr {
                SocketAddr::V4(..) => TcpBuilder::new_v4()?,
                SocketAddr::V6(..) => TcpBuilder::new_v6()?,
            }.to_tcp_stream()?;
            socket.set_nonblocking(true)?;
            match socket.connect(addr) {
                Ok(..) => {}
                Err(ref e) if e.raw_os_error() == Some(EINPROGRESS as i32) => {}
                Err(e) => return Err(e),
            }

            // Wrap the socket in a glib GIOChannel type, and configure the
            // channel to be a raw byte stream.
            let channel = IoChannel::from(socket);
            channel.set_close_on_drop(true);
            channel.set_encoding(None)?;
            channel.set_buffered(false);

            let mut active = IoCondition::new();
            active.input(true).output(true);

            let src = create_source(channel, active, context);

            Ok(TcpStream { inner: src })
        })();

        TcpStreamConnect {
            state: Some(socket),
        }
    }

    /// Blocks the current future's task internally based on the `condition`
    /// specified.
    #[cfg(unix)]
    fn block(&self, condition: &IoCondition) {
        let inner = self.inner.get_ref();
        let mut active = inner.active.borrow_mut();
        if condition.is_input() {
            *inner.read.borrow_mut() = State::Blocked(task::current());
            active.input(true);
        } else {
            *inner.write.borrow_mut() = State::Blocked(task::current());
            active.output(true);
        }

        // Be sure to update the IoCondition that we're interested so we can get
        // events related to this condition.
        let token = inner.token.borrow();
        let token = token.as_ref().unwrap();
        unsafe {
            self.inner.unix_modify_fd(token, &active);
        }
    }

    #[cfg(windows)]
    fn block(&self, condition: &IoCondition) {
        let inner = self.inner.get_ref();
        if condition.is_output() {
            *inner.write.borrow_mut() = State::Blocked(task::current());
        }
    }

    /// Test whether this socket is ready to be read or not.
    ///
    /// If the socket is *not* readable then the current task is scheduled to
    /// get a notification when the socket does become readable. That is, this
    /// is only suitable for calling in a `Future::poll` method and will
    /// automatically handle ensuring a retry once the socket is readable again.
    pub fn poll_read(&self) -> Async<()> {
        // FIXME: probably needs to only check read.
        if self.inner.get_ref().check(&self.inner) {
            Async::Ready(())
        }
        else {
            Async::NotReady
        }
    }

    /// Tests to see if this source is ready to be written to or not.
    ///
    /// If this stream is not ready for a write then `NotReady` will be returned
    /// and the current task will be scheduled to receive a notification when
    /// the stream is writable again. In other words, this method is only safe
    /// to call from within the context of a future's task, typically done in a
    /// `Future::poll` method.
    pub fn poll_write(&self) -> Async<()> {
        // FIXME: probably needs to only check write.
        if self.inner.get_ref().check(&self.inner) {
            Async::Ready(())
        }
        else {
            Async::NotReady
        }
    }
}

impl Read for TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        <&TcpStream>::read(&mut &*self, buf)
    }
}

impl<'a> Read for &'a TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match (&self.inner.get_ref().channel).read(buf) {
            Ok(n) => Ok(n),
            Err(e) => {
                #[cfg(unix)]
                {
                    if e.kind() == io::ErrorKind::WouldBlock {
                        self.block(IoCondition::new().input(true))
                    }
                }
                Err(e)
            }
        }
    }
}

impl Write for TcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        <&TcpStream>::write(&mut &*self, buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        <&TcpStream>::flush(&mut &*self)
    }
}

impl<'a> Write for &'a TcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match (&self.inner.get_ref().channel).write(buf) {
            Ok(n) => Ok(n),
            Err(e) => {
                #[cfg(unix)]
                {
                    if e.kind() == io::ErrorKind::WouldBlock {
                        self.block(IoCondition::new().output(true))
                    }
                }
                Err(e)
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match (&self.inner.get_ref().channel).flush() {
            Ok(n) => Ok(n),
            Err(e) => {
                #[cfg(unix)]
                {
                    if e.kind() == io::ErrorKind::WouldBlock {
                        self.block(IoCondition::new().output(true))
                    }
                }
                Err(e)
            }
        }
    }
}

impl AsyncRead for TcpStream {
    unsafe fn prepare_uninitialized_buffer(&self, _: &mut [u8]) -> bool {
        false
    }

    fn read_buf<B: BufMut>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
        if let Async::NotReady = <TcpStream>::poll_read(self) {
            return Ok(Async::NotReady)
        }
        let r = (&self.inner.get_ref().channel).read(unsafe { buf.bytes_mut() });

        match r {
            Ok(n) => {
                unsafe { buf.advance_mut(n); }
                Ok(Async::Ready(n))
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                self.block(IoCondition::new().input(true));
                Ok(Async::NotReady)
            }
            Err(e) => Err(e),
        }
    }
}

impl AsyncWrite for TcpStream {
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        try_nb!(self.flush());
        Ok(().into())
    }

    fn write_buf<B: Buf>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
        if let Async::NotReady = <TcpStream>::poll_write(self) {
            return Ok(Async::NotReady)
        }
        let r = (&self.inner.get_ref().channel).write(buf.bytes());
        match r {
            Ok(n) => {
                buf.advance(n);
                Ok(Async::Ready(n))
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                self.block(IoCondition::new().output(true));
                Ok(Async::NotReady)
            }
            Err(e) => Err(e),
        }
    }
}

impl Future for TcpStreamConnect {
    type Item = TcpStream;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<TcpStream, io::Error> {
        // Wait for the socket to become writable
        let stream = self.state.take().expect("cannot poll twice")?;
        if stream.inner.get_ref().write.borrow_mut().block() {
            self.state = Some(Ok(stream));
            Ok(Async::NotReady)
        } else {
            // TODO: call take_error() and return that error if one exists
            Ok(stream.into())
        }
    }
}

#[cfg(unix)]
impl SourceFuncs for Inner {
    type CallbackArg = ();

    fn prepare(&self, _source: &Source<Self>) -> (bool, Option<Duration>) {
        (false, None)
    }

    fn check(&self, source: &Source<Self>) -> bool {
        // Test to see whether the events on the fd indicate that we're ready to
        // do some work.
        let token = self.token.borrow();
        let token = token.as_ref().unwrap();
        let ready = unsafe { source.unix_query_fd(token) };

        // TODO: handle hup/error events here as well and translate that to
        //       readable/writable.
        (ready.is_input() && self.read.borrow().is_blocked()) ||
            (ready.is_output() && self.write.borrow().is_blocked())
    }

    fn dispatch(&self,
                source: &Source<Self>,
                _f: glib_sys::GSourceFunc,
                _data: glib_sys::gpointer) -> bool {
        // Learn about how we're ready
        let token = self.token.borrow();
        let token = token.as_ref().unwrap();
        let ready = unsafe { source.unix_query_fd(token) };
        let mut active = self.active.borrow_mut();

        // Wake up the read/write tasks as appropriate
        if ready.is_input() {
            if let Some(task) = self.read.borrow_mut().unblock() {
                task.notify();
            }
            active.input(false);
        }

        if ready.is_output() {
            if let Some(task) = self.write.borrow_mut().unblock() {
                task.notify();
            }
            active.output(false);
        }

        // Configure the active set of conditions we're listening for.
        unsafe {
            source.unix_modify_fd(token, &active);
        }

        true
    }

    fn g_source_func<F>() -> glib_sys::GSourceFunc
        where F: FnMut(Self::CallbackArg) -> bool
    {
        // we never register a callback on this source, so no need to implement
        // this
        panic!()
    }
}
