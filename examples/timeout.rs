extern crate futures;
extern crate futures_glib;
extern crate gtk;

use std::time::Duration;

use futures::Stream;
use futures_glib::{Interval, MainContext};
use futures_glib::future::FuncHandle;

fn main() {
    gtk::init().unwrap();

    let interval = Interval::new(Duration::from_millis(500));
    let interval = interval.for_each(|_| {
        println!("Timeout");
        Ok(())
    });

    let func_handle = FuncHandle::new();
    func_handle.spawn(interval);
    MainContext::default(|main_context| {
        func_handle.attach(main_context);
    });

    gtk::main();
}
