extern crate futures;
extern crate futures_glib;
extern crate gtk;

use std::time::Duration;

use futures::Stream;
use futures_glib::{Interval, MainContext, Executor};
use gtk::{Continue, timeout_add};

fn main() {
    gtk::init().unwrap();

    let cx = MainContext::default(|cx| cx.clone());

    let interval = Interval::new(Duration::from_millis(500));
    let interval = interval.for_each(|_| {
        println!("Timeout");
        Ok(())
    });

    let ex = Executor::new();
    ex.attach(&cx);
    ex.spawn(interval);

    timeout_add(1500, || {
        println!("1");
        gtk::main();
        println!("2");
        Continue(false)
    });

    gtk::main();
}
