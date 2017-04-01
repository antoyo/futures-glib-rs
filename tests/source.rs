extern crate futures_glib;
extern crate glib_sys;

use std::time::Duration;

use futures_glib::{MainContext, MainLoop, Source, SourceFuncs};

struct Quit(MainLoop);

impl SourceFuncs for Quit {
    type CallbackArg = ();

    fn prepare(&self, _source: &Source<Self>) -> (bool, Option<Duration>) {
        self.0.quit();
        (false, None)
    }

    fn check(&self, _source: &Source<Self>) -> bool {
        false
    }

    fn dispatch(&self,
                _source: &Source<Self>,
                _f: glib_sys::GSourceFunc,
                _data: glib_sys::gpointer) -> bool {
        false
    }

    fn g_source_func<F>() -> glib_sys::GSourceFunc
        where F: FnMut(Self::CallbackArg) -> bool
    {
        panic!()
    }
}

#[test]
fn smoke() {
    let cx = MainContext::new();
    let lp = MainLoop::new(Some(&cx));
    let source = Source::new(Quit(lp.clone()));
    source.attach(&cx);
    lp.run();
    source.destroy();
}
