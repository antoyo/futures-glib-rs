use std::cell::RefCell;
use std::iter::Peekable;
use std::sync::{Arc, Weak};
use std::time::Duration;

use futures::{Future, Async};
use futures::executor::{Spawn, Unpark, spawn};
use libc::c_uint;
use slab::Slab;

use super::{MainContext, Source, SourceFuncs};
use stack::{Drain, Stack};

#[derive(Clone)]
pub struct FuncHandle {
    source: Source<Inner>,
}

impl FuncHandle {
    pub fn new() -> Self {
        FuncHandle {
            source: Source::new(Inner::new()),
        }
    }

    pub fn attach(&self, cx: &MainContext) -> c_uint {
        self.source.attach(cx)
    }

    pub fn spawn<F: Future<Item=(), Error=()> + 'static>(&self, future: F) {
        let inner = self.source.get_ref();
        let mut queue = inner.queue.borrow_mut();
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
        inner.ready_queue.push(index);
        if let Some(context) = self.source.context() {
            context.wakeup();
        }
    }
}

impl SourceFuncs for Inner {
    fn prepare(&self, _source: &Source<Self>) -> (bool, Option<Duration>) {
        (false, None)
    }

    fn check(&self, _source: &Source<Self>) -> bool {
        let mut pending = self.pending.borrow_mut();
        assert!(pending.next().is_none());
        *pending = self.ready_queue.drain().peekable();
        pending.peek().is_some()
    }

    fn dispatch<F: FnMut() -> bool>(&self,
                                    source: &Source<Self>,
                                    _callback: F) -> bool {
        let cx = source.context().expect("no context in dispatch");
        for index in self.pending.borrow_mut().by_ref() {
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
        true
    }
}

struct Inner {
    queue: RefCell<Slab<Task>>,
    pending: RefCell<Peekable<Drain<usize>>>,
    ready_queue: Arc<Stack<usize>>,
}

struct Task {
    future: Option<Spawn<Box<Future<Item=(), Error=()>>>>,
    unpark: Option<Arc<Unpark>>,
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