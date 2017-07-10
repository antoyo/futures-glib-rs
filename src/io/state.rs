#[cfg(windows)]
use std::cell::RefCell;
use std::mem;
use std::time::Duration;

use futures::task::{self, Task};
use glib_sys;

use io::{IoChannel, IoCondition};
use {Source, SourceFuncs};

#[derive(Debug)]
pub enum State {
    NotReady,
    Ready,
    Blocked(Task),
}

impl State {
    pub fn block(&mut self) -> bool {
        match *self {
            State::Ready => false,
            State::Blocked(_) |
            State::NotReady => {
                *self = State::Blocked(task::current());
                true
            }
        }
    }

    pub fn unblock(&mut self) -> Option<Task> {
        match mem::replace(self, State::Ready) {
            State::Ready |
            State::NotReady => None,
            State::Blocked(task) => Some(task),
        }
    }

    pub fn is_blocked(&self) -> bool {
        match *self {
            State::Blocked(_) => true,
            _ => false,
        }
    }
}

/// Marker struct on the source returned from `create_watch`.
pub struct IoChannelFuncs {
    #[cfg(windows)]
    pub channel: IoChannel,
    #[cfg(windows)]
    pub write: RefCell<State>,
}

impl IoChannelFuncs {
    #[cfg(windows)]
    pub fn new(channel: IoChannel) -> Self {
        IoChannelFuncs {
            channel: channel,
            write: RefCell::new(State::NotReady),
        }
    }

    #[cfg(unix)]
    pub fn new(_channel: IoChannel) -> Self {
        IoChannelFuncs {
        }
    }
}

impl SourceFuncs for IoChannelFuncs {
    // TODO: this should be &IoChannel and &IoCondition
    type CallbackArg = (IoChannel, IoCondition);

    fn prepare(&self, _source: &Source<Self>) -> (bool, Option<Duration>) {
        panic!()
    }

    fn check(&self, _source: &Source<Self>) -> bool {
        panic!()
    }

    fn dispatch(&self,
                _source: &Source<Self>,
                _fnptr: glib_sys::GSourceFunc,
                _data: glib_sys::gpointer) -> bool {
        panic!()
    }

    fn g_source_func<F>() -> glib_sys::GSourceFunc
        where F: FnMut((IoChannel, IoCondition)) -> bool,
    {
        unsafe extern fn call<F>(channel: *mut glib_sys::GIOChannel,
                                 condition: glib_sys::GIOCondition,
                                 data: glib_sys::gpointer) -> glib_sys::gboolean
            where F: FnMut((IoChannel, IoCondition)) -> bool,
        {
            // TODO: needs a bomb to abort on panic
            let channel = IoChannel {
                inner: glib_sys::g_io_channel_ref(channel),
            };
            let condition = IoCondition { bits: condition };
            if (*(data as *mut F))((channel, condition)) { 1 } else { 0 }
        }

        let call: glib_sys::GIOFunc = Some(call::<F>);

        unsafe {
            mem::transmute(call)
        }
    }

}
