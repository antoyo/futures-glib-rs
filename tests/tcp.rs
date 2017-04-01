extern crate futures_glib;
extern crate futures;
extern crate tokio_io;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use futures::Future;
use futures_glib::net::TcpStream;
use futures_glib::{MainContext, MainLoop, Executor};
use tokio_io::io::{read, write_all, read_to_end};

#[test]
fn smoke() {
    let cx = MainContext::new();
    let lp = MainLoop::new(Some(&cx));
    let ex = Executor::new();
    ex.attach(&cx);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let t = thread::spawn(move || {
        let mut socket = listener.accept().unwrap().0;
        socket.write_all(b"foo").unwrap();
        let mut b = [0; 16];
        assert_eq!(socket.read(&mut b).unwrap(), 4);
        assert_eq!(b[0], 1);
        assert_eq!(b[1], 2);
        assert_eq!(b[2], 3);
        assert_eq!(b[3], 4);
        assert_eq!(b[4], 0);
    });

    let tcp = TcpStream::connect(&addr, &cx);
    let read = tcp.and_then(|s| {
        read(s, [0; 8])
    });
    let done = read.and_then(|(s, buf, n)| {
        assert_eq!(n, 3);
        assert_eq!(buf[0], b'f');
        assert_eq!(buf[1], b'o');
        assert_eq!(buf[2], b'o');
        assert_eq!(buf[3], 0);

        write_all(s, [1, 2, 3, 4])
    });

    let lp2 = lp.clone();
    ex.spawn(done.then(move |_| {
        lp2.quit();
        Ok(())
    }));

    lp.run();
    t.join().unwrap();
    ex.destroy();
}

#[test]
fn write_lots() {
    const N: usize = 16 * 1024;

    let cx = MainContext::new();
    let lp = MainLoop::new(Some(&cx));
    let ex = Executor::new();
    ex.attach(&cx);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let t = thread::spawn(move || {
        let mut socket = listener.accept().unwrap().0;
        let mut n = 0;
        let mut buf = [0; 128];
        while n < N {
            for slot in buf.iter_mut() {
                *slot = 0;
            }
            let amt = socket.read(&mut buf).unwrap();
            n += amt;
            for slot in buf[..amt].iter() {
                assert_eq!(*slot, 1);
            }
        }
    });

    let tcp = TcpStream::connect(&addr, &cx);
    let done = tcp.and_then(|s| {
        write_all(s, vec![1; N])
    });

    let lp2 = lp.clone();
    ex.spawn(done.then(move |_| {
        lp2.quit();
        Ok(())
    }));

    lp.run();
    t.join().unwrap();
    ex.destroy();
}

#[test]
fn read_lots() {
    const N: usize = 16 * 1024;

    let cx = MainContext::new();
    let lp = MainLoop::new(Some(&cx));
    let ex = Executor::new();
    ex.attach(&cx);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let t = thread::spawn(move || {
        let mut socket = listener.accept().unwrap().0;
        let mut n = 0;
        let mut buf = [1; 128];
        while n < N {
            n += socket.write(&mut buf).unwrap();
        }
    });

    let tcp = TcpStream::connect(&addr, &cx);
    let done = tcp.and_then(|s| {
        read_to_end(s, Vec::new())
    });

    let lp2 = lp.clone();
    ex.spawn(done.map(move |(_s, buf)| {
        assert_eq!(buf.len(), N);
        for slot in buf {
            assert_eq!(slot, 1);
        }
        lp2.quit();
    }).map_err(|_| ()));

    lp.run();
    t.join().unwrap();
    ex.destroy();
}

