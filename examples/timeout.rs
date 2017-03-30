extern crate futures;
extern crate futures_glib;

use std::time::Duration;

use futures::Future;
use futures_glib::{Executor, MainContext, MainLoop, Timeout};

fn main() {
    futures_glib::init();

    let cx = MainContext::default(|cx| cx.clone());
    let lp = MainLoop::new(None);

    let timeout = Timeout::new(Duration::from_millis(500));
    let lp2 = lp.clone();
    let timeout = timeout.and_then(move |_| {
        println!("Timeout");
        lp2.quit();
        Ok(())
    });

    let ex = Executor::new();
    ex.attach(&cx);
    ex.spawn(timeout);

    lp.run();

    ex.destroy();
}
