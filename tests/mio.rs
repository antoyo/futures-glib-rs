extern crate mio;

use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::io::AsRawFd;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
use std::time::Duration;

use mio::{Events, Poll, PollOpt, Ready, Token};
use mio::tcp::TcpStream;
use mio::unix::EventedFd;

// A token is used to identify an event.
const EVENT_LOOP: Token = Token(2);
const SOCKET: Token = Token(0);

#[test]
fn test_recursive_mio() {
    let addr = "172.217.4.238:80".parse().unwrap();
    let mut stream = TcpStream::connect(&addr).unwrap();

    let outer_poll = Poll::new().unwrap();

    // Create an object to monitor events.
    let poll = Poll::new().unwrap();

    // Register for read event on the socket.
    poll.register(&stream, SOCKET, Ready::readable() | Ready::writable(), PollOpt::edge()).unwrap();

    outer_poll.register(&EventedFd(&poll.as_raw_fd()), EVENT_LOOP, Ready::readable() | Ready::writable(), PollOpt::edge()).unwrap();

    // Create a collection where the ready events will go.
    let mut outer_events = Events::with_capacity(1);
    let mut events = Events::with_capacity(1);
    let mut written = false;
    let mut buffer = [0; 4096];

    'outer:
    loop {
        // Wait for events.
        outer_poll.poll(&mut outer_events, None).unwrap();

        for event in outer_events.iter() {
            match event.token() {
                EVENT_LOOP => {
                    poll.poll(&mut events, Some(Duration::from_millis(0))).unwrap();
                    for event in events.iter() {
                        match event.token() {
                            SOCKET => {
                                if event.readiness() & Ready::writable() == Ready::writable() {
                                    if !written {
                                        stream.write(b"GET / HTTP/1.1\n\n").unwrap();
                                        written = true;
                                    }
                                }
                                if event.readiness() & Ready::readable() == Ready::readable() {
                                    stream.read(&mut buffer).unwrap();
                                    print!("{}", String::from_utf8_lossy(&buffer));
                                    assert_eq!(&buffer[..4], b"HTTP");
                                    break 'outer;
                                }
                            },
                            _ => unreachable!(),
                        }
                    }
                },
                _ => unreachable!(),
            }
        }
    }
}
