extern crate futures;
extern crate futures_glib;
// extern crate gtk;

use std::time::Duration;

use futures::Stream;
use futures_glib::{Interval, MainContext, MainLoop};
use futures_glib::future::FuncHandle;

fn main() {
    let cx = MainContext::default(|cx| cx.clone());
    let lp = MainLoop::new(&cx);

    let interval = Interval::new(Duration::from_millis(500));
    let interval = interval.for_each(|_| {
        println!("Timeout");
        Ok(())
    });

    let func_handle = FuncHandle::new();
    func_handle.attach(&cx);
    func_handle.spawn(interval);

    lp.run();
}
