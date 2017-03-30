extern crate futures;
extern crate futures_glib;

use std::thread;

use futures::Future;
use futures::future::ok;
use futures_glib::{Executor, MainContext, MainLoop};

fn main() {
    futures_glib::init();

    let cx = MainContext::default(|cx| cx.clone());
    let lp = MainLoop::new(None);

    let ex = Executor::new();
    ex.attach(&cx);

    let remote = ex.remote();

    thread::spawn(move || {
        let future = ok(())
            .and_then(|_| {
                println!("Done");
                Ok(())
            });

        remote.spawn(|ex: Executor| {
            let f = ok(())
                .and_then(|_| {
                    println!("Second");
                    Ok(())
                });
            ex.spawn(f);
            future
        });
    });

    lp.run();
}
