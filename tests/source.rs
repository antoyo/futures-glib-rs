extern crate futures_glib;

use std::time::Duration;

use futures_glib::{MainContext, MainLoop, Source, SourceFuncs};

struct Quit(MainLoop);

impl SourceFuncs for Quit {
    fn prepare(&self, _source: &Source<Self>) -> (bool, Option<Duration>) {
        self.0.quit();
        (false, None)
    }

    fn check(&self, _source: &Source<Self>) -> bool {
        false
    }

    fn dispatch<F: FnMut() -> bool>(&self,
                                    _source: &Source<Self>,
                                    _callback: F) -> bool {
        false
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
