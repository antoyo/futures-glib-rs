extern crate futures_glib;

use futures_glib::{MainLoop, MainContext};

#[test]
fn smoke() {
    let cx = MainContext::new();
    let lp = MainLoop::new(Some(&cx));
    assert!(!lp.is_running());
    lp.context();
}
