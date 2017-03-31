extern crate futures_glib;

use std::thread;

use futures_glib::MainContext;

#[test]
fn smoke() {
    MainContext::default(|_| ());
}

#[test]
fn locking() {
    let cx = MainContext::new();
    let cx2 = cx.clone();
    assert!(!cx.is_owner());
    let locked = cx.try_lock().unwrap();
    assert!(!locked.pending());
    assert!(cx2.is_owner());
    thread::spawn(move || {
        assert!(!cx2.is_owner());
        assert!(cx2.try_lock().is_none());
    }).join().unwrap();
    drop(locked);
    cx.try_lock().unwrap();
    assert!(!cx.is_owner());
}

#[test]
fn wakeup() {
    let cx = MainContext::new();
    cx.wakeup();
}

#[test]
fn thread_default() {
    let cx = MainContext::new();
    {
        let _scope = cx.push_thread_default();
        MainContext::default(|cx2| {
            let locked = cx2.try_lock().unwrap();
            assert!(cx.is_owner());
            drop(locked);
        });
        assert!(cx.try_lock().is_some());
    }
    MainContext::default(|cx2| {
        let locked = cx2.try_lock().unwrap();
        assert!(!cx.is_owner());
        drop(locked);
    });
}
