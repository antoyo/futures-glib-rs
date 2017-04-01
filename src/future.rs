use std::cell::RefCell;
use std::iter::Peekable;
use std::sync::{Arc, Weak};
use std::time::Duration;

use futures::executor::{Spawn, Unpark, spawn};
use futures::future;
use futures::sync::mpsc;
use futures::{Future, IntoFuture, Async};
use glib_sys;
use slab::Slab;

use super::{MainContext, Source, SourceFuncs};
use stack::{Drain, Stack};

use self::Message::*;

/// A handle through which futures can be executed.
///
/// This structure is an instance of a `Source` which can be used to manage the
/// execution of a number of futures within.
#[derive(Clone)]
pub struct Executor {
    source: Source<Inner>,
}

impl Executor {
    /// Creates a new executor unassociated with any context ready to start
    /// spawning futures.
    pub fn new() -> Self {
        Executor {
            source: Source::new(Inner::new()),
        }
    }

    /// Attaches this executor to a context, returning the token that it was
    /// assigned.
    ///
    /// This is required to be called for futures to be completed.
    pub fn attach(&self, cx: &MainContext) {
        cx.wakeup();
        self.source.attach(cx)
    }

    /// Generates a remote handle to this event loop which can be used to spawn tasks from other threads into this event loop.
    pub fn remote(&self) -> Remote {
        let inner = self.source.get_ref();
        Remote {
            tx: inner.tx.clone(),
        }
    }

    /// Unregister this executor and free up internal resources.
    pub fn destroy(&self) {
        self.source.destroy()
    }

    /// Spawns a new future onto the event loop that this source is associated
    /// with.
    ///
    /// This function is given a future which is then spawned onto the glib
    /// event loop. The glib event loop will listen for incoming events of when
    /// futures are ready and attempt to push them all to completion.
    ///
    /// The futures spawned here will not be completed unless the `attach`
    /// function is called above.
    pub fn spawn<F: Future<Item=(), Error=()> + 'static>(&self, future: F) {
        let inner = self.source.get_ref();
        inner.spawn(future, &self.source);
    }

    /// Same as `spawn` above, but spawns a function that returns a future
    pub fn spawn_fn<F, R>(&self, f: F)
        where F: FnOnce() -> R + 'static,
              R: IntoFuture<Item=(), Error=()> + 'static,
    {
        self.spawn(future::lazy(f))
    }
}

impl SourceFuncs for Inner {
    type CallbackArg = ();

    fn prepare(&self, _source: &Source<Self>) -> (bool, Option<Duration>) {
        (false, None)
    }

    fn check(&self, _source: &Source<Self>) -> bool {
        let mut pending = self.pending.borrow_mut();
        assert!(pending.next().is_none());
        *pending = self.ready_queue.drain().peekable();
        pending.peek().is_some()
    }

    fn dispatch(&self,
                source: &Source<Self>,
                _fnptr: glib_sys::GSourceFunc,
                _data: glib_sys::gpointer) -> bool {
        let cx = source.context().expect("no context in dispatch");
        for index in self.pending.borrow_mut().by_ref() {
            if index == self.id {
                continue;
            }
            let (task, wake) = {
                let mut queue = self.queue.borrow_mut();
                let slot = &mut queue[index];
                if slot.unpark.is_none() {
                    slot.unpark = Some(Arc::new(MyUnpark {
                        id: index,
                        ready_queue: Arc::downgrade(&self.ready_queue),
                        main_context: cx.clone(),
                    }));
                }
                (slot.future.take(), slot.unpark.as_ref().unwrap().clone())
            };
            let mut task = match task {
                Some(future) => future,
                None => continue,
            };
            let res = task.poll_future(wake);
            let mut queue = self.queue.borrow_mut();
            match res {
                Ok(Async::NotReady) => { queue[index].future = Some(task); }
                Ok(Async::Ready(())) |
                Err(()) => { queue.remove(index).unwrap(); }
            }
        }
        loop {
            let wake = {
                let mut queue = self.queue.borrow_mut();
                let slot = &mut queue[self.id];
                if slot.unpark.is_none() {
                    slot.unpark = Some(Arc::new(MyUnpark {
                        id: self.id,
                        ready_queue: Arc::downgrade(&self.ready_queue),
                        main_context: cx.clone(),
                    }));
                }
                slot.unpark.as_ref().unwrap().clone()
            };
            if let Ok(Async::Ready(Some(Run(r)))) = (*self.message_queue.borrow_mut()).poll_stream(wake) {
                r.call_box(source);
            }
            else {
                break;
            }
        }
        true
    }

    fn g_source_func<F>() -> glib_sys::GSourceFunc
        where F: FnMut(()) -> bool,
    {
        unsafe extern fn call<F>(data: glib_sys::gpointer) -> glib_sys::gboolean
            where F: FnMut(()) -> bool,
        {
            // TODO: needs a bomb to abort on panic
            if (*(data as *mut F))(()) { 1 } else { 0 }
        }

        Some(call::<F>)
    }
}

struct Inner {
    id: usize,
    queue: RefCell<Slab<Task>>,
    pending: RefCell<Peekable<Drain<usize>>>,
    ready_queue: Arc<Stack<usize>>,
    message_queue: RefCell<Spawn<mpsc::UnboundedReceiver<Message>>>,
    tx: mpsc::UnboundedSender<Message>,
}

struct Task {
    future: Option<Spawn<Box<Future<Item=(), Error=()>>>>,
    unpark: Option<Arc<Unpark>>,
}

impl Inner {
    fn new() -> Self {
        let (tx, rx) = mpsc::unbounded();
        let mut queue = Slab::with_capacity(128);
        let id = {
            let entry = queue.vacant_entry().unwrap();
            let index = entry.index();
            entry.insert(Task {
                unpark: None,
                future: None,
            });
            index
        };
        let ready_queue = Stack::new();
        ready_queue.push(id);
        Inner {
            id: id,
            queue: RefCell::new(queue),
            pending: RefCell::new(Stack::new().drain().peekable()),
            ready_queue: Arc::new(ready_queue),
            message_queue: RefCell::new(spawn(rx)),
            tx: tx,
        }
    }

    fn spawn<F: Future<Item=(), Error=()> + 'static>(&self, future: F, source: &Source<Inner>) {
        let mut queue = self.queue.borrow_mut();
        if queue.vacant_entry().is_none() {
            let len = queue.len();
            queue.reserve_exact(len);
        }
        let entry = queue.vacant_entry().unwrap();
        let index = entry.index();
        entry.insert(Task {
            unpark: None,
            future: Some(spawn(Box::new(future))),
        });
        self.ready_queue.push(index);
        if let Some(context) = source.context() {
            context.wakeup();
        }
    }
}

struct MyUnpark {
    id: usize,
    main_context: MainContext,
    ready_queue: Weak<Stack<usize>>,
}

impl Unpark for MyUnpark {
    fn unpark(&self) {
        if let Some(queue) = self.ready_queue.upgrade() {
            queue.push(self.id);
            self.main_context.wakeup();
        }
    }
}

/// Handle to an event loop, used to construct I/O objects, send messages, and otherwise interact indirectly with the event loop itself.
///
/// Handles can be cloned, and when cloned they will still refer to the same underlying event loop.
#[derive(Clone)]
pub struct Remote {
    tx: mpsc::UnboundedSender<Message>,
}

impl Remote {
    /// Spawns a new future into the event loop this remote is associated with.
    ///
    /// This function takes a closure which is executed within the context of the I/O loop itself. The future returned by the closure will be scheduled on the event loop an run to completion.
    ///
    /// Note that while the closure, F, requires the Send bound as it might cross threads, the future R does not.
    pub fn spawn<F, R>(&self, f: F)
        where F: FnOnce(Executor) -> R + Send + 'static,
              R: IntoFuture<Item=(), Error=()>,
              R::Future: 'static
    {
        self.tx.send(Run(Box::new(|source: &Source<Inner>| {
            let inner = source.get_ref();
            let f = f(Executor {
                source: source.clone(),
            });
            inner.spawn(f.into_future(), source);
        }))).unwrap();
    }
}

enum Message {
    Run(Box<FnBox>),
}

trait FnBox: Send + 'static {
    fn call_box(self: Box<Self>, source: &Source<Inner>);
}

impl<F: FnOnce(&Source<Inner>) + Send + 'static> FnBox for F {
    fn call_box(self: Box<Self>, source: &Source<Inner>) {
        (*self)(source)
    }
}
