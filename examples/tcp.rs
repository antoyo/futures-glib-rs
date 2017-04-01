extern crate futures;
extern crate futures_glib;
extern crate tokio_io;

use std::net::ToSocketAddrs;

use futures::Future;
use futures_glib::net::TcpStream;
use futures_glib::{Executor, MainContext, MainLoop};
use tokio_io::io;

fn main() {
    futures_glib::init();

    let addr = "google.com:80".to_socket_addrs().unwrap().next().unwrap();

    let cx = MainContext::default(|cx| cx.clone());
    let lp = MainLoop::new(None);
    let ex = Executor::new();
    ex.attach(&cx);

    let tcp = TcpStream::connect(&addr, &cx);
    let tcp = tcp.and_then(|tcp| {
        io::write_all(tcp, "\
            GET / HTTP/1.0\r\n\
            Host: www.google.com\r\n\
            \r\n\
        ".as_bytes()).map(|p| p.0).and_then(io::flush)
    });
    let tcp = tcp.and_then(|tcp| {
        io::read_to_end(tcp, Vec::new()).map(|p| p.1)
    });

    let lp2 = lp.clone();
    ex.spawn(tcp.then(move |res| {
        match res {
            Ok(res) => println!("{}", String::from_utf8_lossy(&res)),
            Err(e) => println!("error: {}", e),
        }
        lp2.quit();
        Ok(())
    }));

    lp.run();
    ex.destroy();
}
