extern crate futures;
extern crate futures_glib;

use std::time::Duration;

use futures::Stream;
use futures_glib::{Interval, MainContext, MainLoop, Executor};

fn main() {
    futures_glib::init();

    let cx = MainContext::default(|cx| cx.clone());
    let lp = MainLoop::new(None);

    let interval = Interval::new(Duration::from_millis(500));
    let interval = interval.for_each(|_| {
        println!("Interval");
        Ok(())
    });

    let ex = Executor::new();
    ex.attach(&cx);
    ex.spawn(interval);

    lp.run();
}
