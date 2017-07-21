use std::cell::RefCell;
use std::io;
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
use std::time::{Duration, Instant};

use futures::{Async, Future, Stream, task};
use futures::future::{Executor, ExecuteError};
use glib_sys;
use tokio_core::reactor::{self, Handle};

#[cfg(unix)]
use UnixToken;
use {MainContext, Source, SourceFuncs};
use io::IoCondition;
use io::state::State;

pub struct Core {
    source: Source<Inner>,
}

impl Core {
    #[cfg(windows)]
    pub fn new(cx: &MainContext) -> Result<Self, io::Error> {
        unimplemented!();
    }

    #[cfg(unix)]
    pub fn new(cx: &MainContext) -> Result<Self, io::Error> {
        let core = reactor::Core::new()?;
        // TODO: windows.
        let fd = core.as_raw_fd();
        let mut active = IoCondition::new();
        active.input(true).output(true);
        let source = Source::new(Inner {
            core: RefCell::new(core),
            active: RefCell::new(active.clone()),
            read: RefCell::new(State::NotReady),
            write: RefCell::new(State::NotReady),
            token: RefCell::new(None),
        });
        source.attach(cx);
        let t = source.unix_add_fd(fd, &active);
        *source.get_ref().token.borrow_mut() = Some(t);
        Ok(Core {
            source,
        })
    }

    fn block(&self, condition: &IoCondition) {
        let inner = self.source.get_ref();
        let mut active = inner.active.borrow_mut();
        if condition.is_input() {
            *inner.read.borrow_mut() = State::Blocked(task::current());
            active.input(true);
        } else {
            *inner.write.borrow_mut() = State::Blocked(task::current());
            active.output(true);
        }

        #[cfg(unix)]
        {
            // Be sure to update the IoCondition that we're interested so we can get
            // events related to this condition.
            let token = inner.token.borrow();
            let token = token.as_ref().unwrap();
            unsafe {
                self.source.unix_modify_fd(token, &active);
            }
        }
    }

    pub fn handle(&self) -> Handle {
        self.source.get_ref().core.borrow().handle()
    }
}

struct Inner {
    core: RefCell<reactor::Core>,
    active: RefCell<IoCondition>,
    #[cfg(unix)]
    token: RefCell<Option<UnixToken>>,
    read: RefCell<State>,
    write: RefCell<State>,
}

impl SourceFuncs for Inner {
    type CallbackArg = ();

    fn prepare(&self, _source: &Source<Self>) -> (bool, Option<Duration>) {
        (false, None)
    }

    fn check(&self, source: &Source<Self>) -> bool {
        self.core.borrow_mut().has_events()
    }

    fn dispatch(&self,
                source: &Source<Self>,
                _fnptr: glib_sys::GSourceFunc,
                _data: glib_sys::gpointer) -> bool {
        // Learn about how we're ready
        #[cfg(unix)]
        {
            let token = self.token.borrow();
            let token = token.as_ref().unwrap();
            let ready = unsafe { source.unix_query_fd(token) };
            let mut active = self.active.borrow_mut();

            // Wake up the read/write tasks as appropriate
            if ready.is_input() {
                if let Some(task) = self.read.borrow_mut().unblock() {
                    task.notify();
                }
            }

            if ready.is_output() {
                if let Some(task) = self.write.borrow_mut().unblock() {
                    task.notify();
                }
            }

            // Configure the active set of conditions we're listening for.
            unsafe {
                source.unix_modify_fd(token, &active);
            }
        }
        let start = Instant::now();
        self.core.borrow_mut().process(start);
        true
    }

    fn g_source_func<F>() -> glib_sys::GSourceFunc
        where F: FnMut(()) -> bool,
    {
        unimplemented!();
    }
}

impl<F> Executor<F> for Core
    where F: Future<Item = (), Error = ()> + 'static,
{
    fn execute(&self, future: F) -> Result<(), ExecuteError<F>> {
        self.source.get_ref().core.borrow().execute(future)
    }
}

impl Stream for Core {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Result<Async<Option<Self::Item>>, Self::Error> {
        // FIXME: can be read or write.
        if self.source.get_ref().read.borrow_mut().block() {
            Ok(Async::NotReady)
        } else {
            *self.source.get_ref().read.borrow_mut() = State::NotReady; // TODO: remove this line.
            // TODO: call take_error() and return that error if one exists
            Ok(Async::Ready(Some(())))
        }
    }
}
