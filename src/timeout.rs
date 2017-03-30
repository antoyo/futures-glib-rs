use std::sync::Arc;
use std::time::Duration;

use futures::{Async, Stream};
use futures::executor::{Spawn, Unpark, spawn};
use futures::sync::mpsc;
use glib_sys;
use glib_sys::{gboolean, gpointer, g_timeout_add_full};
use libc::c_uint;

pub struct Interval {
    id: c_uint,
    rx: mpsc::Receiver<()>,
}

impl Interval {
    pub fn new(duration: Duration) -> Self {
        let (tx, rx) = mpsc::channel(0);
        let tx = Box::into_raw(Box::new(spawn(tx)));
        let id = unsafe { g_timeout_add_full(glib_sys::G_PRIORITY_DEFAULT, millis(duration) as u32, Some(handler),
            tx as gpointer, Some(destroy)) };
        Interval {
            id: id,
            rx: rx,
        }
    }
}

impl Drop for Interval {
    fn drop(&mut self) {
        println!("source remove");
        unsafe { glib_sys::g_source_remove(self.id) };
    }
}

impl Stream for Interval {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Result<Async<Option<Self::Item>>, Self::Error> {
        self.rx.poll()
    }
}

const NANOS_PER_MILLI: u32 = 1_000_000;
const MILLIS_PER_SEC: u64 = 1_000;

pub fn millis(duration: Duration) -> u64 {
    // Round up.
    let millis = (duration.subsec_nanos() + NANOS_PER_MILLI - 1) / NANOS_PER_MILLI;
    duration.as_secs().saturating_mul(MILLIS_PER_SEC).saturating_add(millis as u64)
}

unsafe extern fn destroy(data: gpointer) {
    let _ = Box::from_raw(data as *mut Spawn<mpsc::Sender<()>>);
}

unsafe extern fn handler(data: gpointer) -> gboolean {
    let tx = data as *mut Spawn<mpsc::Sender<()>>;
    let no_op = Arc::new(NoOp {}) as Arc<Unpark>;
    drop((*tx).start_send((), &no_op));
    1
}

struct NoOp {
}

impl Unpark for NoOp {
    fn unpark(&self) {}
}
