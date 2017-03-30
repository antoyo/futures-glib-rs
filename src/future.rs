use std::cell::RefCell;
use std::iter::Peekable;
use std::sync::Arc;

use futures::Future;
use futures::executor::{Spawn, Unpark, spawn};
use libc::{c_int, c_uint};
use slab::Slab;

use super::{MainContext, Source, SourceFuncs};
use stack::{Drain, Stack};

pub struct FuncHandle {
    source: Source<Inner>,
}

impl FuncHandle {
    pub fn new() -> Self {
        FuncHandle {
            source: Source::new(Inner::new()),
        }
    }

    pub fn attach(&self, main_context: &MainContext) -> c_uint {
        self.source.attach(&main_context)
    }

    pub fn spawn<F: Future<Item=(), Error=()> + 'static>(&self, future: F) {
        let inner = self.source.get_ref();
        let mut queue = inner.queue.borrow_mut();
        if queue.vacant_entry().is_none() {
            let len = queue.len();
            queue.reserve_exact(len);
        }
        let entry = queue.vacant_entry().unwrap();
        let entry = entry.insert(spawn(Box::new(future)));
        inner.ready_queue.push(entry.index());
        if let Some(context) = self.source.get_context() {
            context.wakeup();
        }
    }
}

impl SourceFuncs for Inner {
    fn prepare(&self, source: &Source<Self>, timeout: &mut c_int) -> bool {
        false
    }

    fn check(&self, source: &Source<Self>) -> bool {
        let mut pending = self.pending.borrow_mut();
        *pending = self.ready_queue.drain().peekable();
        pending.peek().is_some()
    }

    fn dispatch<F: FnMut() -> bool>(&self, source: &Source<Self>, callback: F) -> bool {
        for index in self.pending.borrow_mut().by_ref() {
            let unpark = Arc::new(MyUnpark {
                id: index,
                main_context: source.get_context().unwrap(),
                ready_queue: self.ready_queue.clone(),
            });
            self.queue.borrow_mut()[index].poll_future(unpark);
        }
        true
    }
}

struct Inner {
    queue: RefCell<Slab<Spawn<Box<Future<Item=(), Error=()>>>>>,
    pending: RefCell<Peekable<Drain<usize>>>,
    ready_queue: Arc<Stack<usize>>,
}

impl Inner {
    fn new() -> Self {
        Inner {
            queue: RefCell::new(Slab::with_capacity(128)),
            pending: RefCell::new(Stack::new().drain().peekable()),
            ready_queue: Arc::new(Stack::new()),
        }
    }
}

struct MyUnpark {
    id: usize,
    main_context: MainContext,
    ready_queue: Arc<Stack<usize>>,
}

impl Unpark for MyUnpark {
    fn unpark(&self) {
        self.ready_queue.push(self.id);
        self.main_context.wakeup();
    }
}
