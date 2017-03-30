use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use futures::{Async, Future};
use futures::unsync::oneshot;
use glib_sys;
use glib_sys::{gboolean, gpointer, g_timeout_add_full};
use libc::c_uint;

use utils::millis;

pub struct Timeout {
    id: c_uint,
    rx: oneshot::Receiver<()>,
    triggered: Rc<Cell<bool>>,
}

impl Timeout {
    pub fn new(duration: Duration) -> Self {
        assert_initialized_main_thread!();
        let triggered = Rc::new(Cell::new(false));
        let (tx, rx) = oneshot::channel();
        let tx = Box::into_raw(Box::new(Some((tx, triggered.clone()))));
        let id = unsafe { g_timeout_add_full(glib_sys::G_PRIORITY_DEFAULT, millis(duration) as u32, Some(handler),
            tx as gpointer, Some(destroy)) };
        Timeout {
            id: id,
            rx: rx,
            triggered: triggered,
        }
    }
}

impl Drop for Timeout {
    fn drop(&mut self) {
        if !self.triggered.get() {
            unsafe { glib_sys::g_source_remove(self.id) };
        }
    }
}

impl Future for Timeout {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Result<Async<()>, ()> {
        self.rx.poll()
            .map_err(|_| ())
    }
}

unsafe extern fn handler(data: gpointer) -> gboolean {
    let tuple = data as *mut Option<(oneshot::Sender<()>, Rc<Cell<bool>>)>;
    let (tx, triggered) = (*tuple).take().unwrap();
    triggered.set(true);
    drop(tx.send(()));
    0
}

pub unsafe extern fn destroy(data: gpointer) {
    let _ = Box::from_raw(data as *mut Option<(oneshot::Sender<()>, Rc<Cell<bool>>)>);
}
