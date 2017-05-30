extern crate futures;
extern crate futures_glib;

use std::thread;

use futures::Future;
use futures::sync::oneshot;
use futures_glib::{MainContext, MainLoop, Executor};

#[test]
fn smoke() {
    let cx = MainContext::new();
    let lp = MainLoop::new(Some(&cx));
    let e = Executor::new();
    e.attach(&cx);

    let lp2 = lp.clone();
    e.spawn_fn(move || {
        lp2.quit();
        Ok(())
    });

    lp.run();
    e.destroy();
}

#[test]
fn oneshot() {
    let cx = MainContext::new();
    let lp = MainLoop::new(Some(&cx));
    let e = Executor::new();
    e.attach(&cx);

    let (tx, rx) = oneshot::channel();
    let lp2 = lp.clone();
    e.spawn(rx.then(move |_| {
        lp2.quit();
        Ok(())
    }));

    let t = thread::spawn(|| tx.send(()).unwrap());

    lp.run();
    e.destroy();
    t.join().unwrap();
}

#[test]
fn oneshot2() {
    let cx = MainContext::new();
    let lp = MainLoop::new(Some(&cx));
    let e = Executor::new();
    e.attach(&cx);

    let lp2 = lp.clone();
    e.spawn_fn(move || {
        let (tx, rx) = oneshot::channel();
        let t = thread::spawn(|| tx.send(()).unwrap());
        rx.then(move |_| {
            lp2.quit();
            t.join().unwrap();
            Ok(())
        })
    });

    lp.run();
    e.destroy();
}

#[test]
fn oneshot_many() {
    let cx = MainContext::new();
    let lp = MainLoop::new(Some(&cx));
    let e = Executor::new();
    e.attach(&cx);

    let (tx1, rx1) = oneshot::channel();
    let (tx2, rx2) = oneshot::channel();
    let lp2 = lp.clone();

    let rx1 = rx1.then(|r| {
        thread::spawn(|| tx2.send(()).unwrap());
        r
    });
    e.spawn(rx1.join(rx2).then(move |_| {
        lp2.quit();
        Ok(())
    }));

    let t = thread::spawn(|| tx1.send(()).unwrap());

    lp.run();
    e.destroy();
    t.join().unwrap();
}

#[test]
fn spawn_in_pol() {
    let cx = MainContext::new();
    let lp = MainLoop::new(Some(&cx));
    let e = Executor::new();
    e.attach(&cx);

    let e2 = e.clone();
    let lp2 = lp.clone();
    e.spawn_fn(move || {
        e2.spawn_fn(move || {
            lp2.quit();
            Ok(())
        });
        Ok(())
    });

    lp.run();
    e.destroy();
}

#[test]
fn unpark_after_done() {
    use futures::task;

    let cx = MainContext::new();
    let lp = MainLoop::new(Some(&cx));
    let e = Executor::new();
    e.attach(&cx);

    let e2 = e.clone();
    let lp2 = lp.clone();
    e.spawn_fn(move || {
        let task = task::current();
        e2.spawn_fn(move || {
            task.notify();
            lp2.quit();
            Ok(())
        });
        Ok(())
    });

    lp.run();
    e.destroy();
}
