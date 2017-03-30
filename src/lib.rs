//! A crate to bring a `futures` executor to the glib event loop

extern crate futures;
extern crate glib_sys;
extern crate libc;
extern crate slab;

mod future;
mod interval;
#[macro_use]
mod rt;
mod stack;
mod timeout;
mod utils;

use std::cmp;
use std::marker;
use std::mem;
use std::ptr;
use std::rc::Rc;
use std::time::{Duration, Instant};

use libc::{c_int, c_uint};

pub use future::Executor;
pub use interval::Interval;
pub use rt::init;
pub use timeout::Timeout;

const FALSE: c_int = 0;
const TRUE: c_int = !FALSE;

/// A small type to avoid running the destructor of `T`
struct ManuallyDrop<T> { inner: Option<T> }

impl<T> ManuallyDrop<T> {
    fn new(t: T) -> ManuallyDrop<T> {
        ManuallyDrop { inner: Some(t) }
    }

    fn get_ref(&self) -> &T {
        self.inner.as_ref().unwrap()
    }
}

impl<T> Drop for ManuallyDrop<T> {
    fn drop(&mut self) {
        mem::forget(self.inner.take())
    }
}

/// Binding to the underlying `GMainContext` type.
pub struct MainContext {
    inner: *mut glib_sys::GMainContext,
}

unsafe impl Send for MainContext {}
unsafe impl Sync for MainContext {}

impl MainContext {
    /// Creates a new context to execute within.
    pub fn new() -> MainContext {
        unsafe {
            let ptr = glib_sys::g_main_context_new();
            assert!(!ptr.is_null());
            MainContext { inner: ptr }
        }
    }

    /// Acquires a reference to this thread's default context.
    ///
    /// This is the main context used for main loop functions when a main loop
    /// is not explicitly specified, and corresponds to the "main" main loop.
    pub fn default<F, R>(f: F) -> R
        where F: FnOnce(&MainContext) -> R
    {
        unsafe {
            let ptr = glib_sys::g_main_context_default();
            assert!(!ptr.is_null());
            let cx = ManuallyDrop::new(MainContext { inner: ptr });
            f(cx.get_ref())

        }
    }

    /// Gets the thread-default `MainContext` for this thread.
    ///
    /// Asynchronous operations that want to be able to be run in contexts other
    /// than the default one should call this method or to get a `MainContext`
    /// to add their `Source`s to. (Note that even in single-threaded programs
    /// applications may sometimes want to temporarily push a non-default
    /// context, so it is not safe to assume that this will always yield `None`
    /// if you are running in the default thread.)
    pub fn thread_default<F, R>(f: F) -> R
        where F: FnOnce(Option<&MainContext>) -> R
    {
        unsafe {
            let ptr = glib_sys::g_main_context_get_thread_default();
            if ptr.is_null() {
                f(None)
            } else {
                let cx = ManuallyDrop::new(MainContext { inner: ptr });
                f(Some(cx.get_ref()))
            }
        }
    }

    /// Attempts to become the owner of the specified context.
    ///
    /// If some other thread is the owner of the context, returns `Err`
    /// immediately. Ownership is properly recursive: the owner can require
    /// ownership again.
    ///
    /// If the context is successfully locked then a locked version is
    /// returned, otherwise an `Err` is returned with this context.
    pub fn try_lock(self) -> Result<LockedMainContext, MainContext> {
        if unsafe { glib_sys::g_main_context_acquire(self.inner) } == TRUE {
            Ok(LockedMainContext { inner: self, _marker: marker::PhantomData })
        } else {
            Err(self)
        }
    }

    /// Determines whether this thread holds the (recursive) ownership of this
    /// `MainContext`.
    ///
    /// This is useful to know before waiting on another thread that may be
    /// blocking to get ownership of context .
    pub fn is_owner(&self) -> bool {
        unsafe { glib_sys::g_main_context_is_owner(self.inner) == TRUE }
    }

    /// If context is currently blocking in `iteration` waiting for a source to
    /// become ready, cause it to stop blocking and return.  Otherwise, cause
    /// the next invocation of `iteration` to return without blocking.
    ///
    /// This API is useful for low-level control over `MainContext`; for
    /// example, integrating it with main loop implementations such as
    /// `MainLoop`.
    ///
    /// Another related use for this function is when implementing a main loop
    /// with a termination condition, computed from multiple threads.
    pub fn wakeup(&self) {
        unsafe {
            glib_sys::g_main_context_wakeup(self.inner)
        }
    }

    /// Acquires context and sets it as the thread-default context for the
    /// current thread.
    ///
    /// This will cause certain asynchronous operations (such as most gio-based
    /// I/O) which are started in this thread to run under context and deliver
    /// their results to its main loop, rather than running under the global
    /// default context in the main thread. Note that calling this function
    /// changes the context returned by `thread_default` not the one returned
    /// by `default`.
    ///
    /// Normally you would call this function shortly after creating a new
    /// thread, passing it a `MainContext` which will be run by a `MainLoop` in
    /// that thread, to set a new default context for all async operations in
    /// that thread.
    ///
    /// If you don't have control over how the new thread was created (e.g. in
    /// the new thread isn't newly created, or if the thread life cycle is
    /// managed by a `ThreadPool`), it is always suggested to wrap the logic
    /// that needs to use the new `MainContext` inside `push_thread_default` block.
    /// otherwise threads that are re-used will end up never explicitly
    /// releasing the `MainContext` reference they hold.
    ///
    /// In some cases you may want to schedule a single operation in a
    /// non-default context, or temporarily use a non-default context in the
    /// main thread. In that case, you can wrap the call to the asynchronous
    /// operation inside a `push_thread_default` block, but it is up to you to
    /// ensure that no other asynchronous operations accidentally get started
    /// while the non-default context is active.
    ///
    /// This context will be popped from the default scope when the returned
    /// `PushThreadDefault` value goes out of scope.
    pub fn push_thread_default(&self) -> PushThreadDefault {
        unsafe {
            glib_sys::g_main_context_push_thread_default(self.inner);
        }
        PushThreadDefault { inner: self }
    }
}

impl Clone for MainContext {
    fn clone(&self) -> MainContext {
        unsafe {
            let ptr = glib_sys::g_main_context_ref(self.inner);
            assert!(!ptr.is_null());
            MainContext { inner: ptr }
        }
    }
}

impl Drop for MainContext {
    fn drop(&mut self) {
        unsafe {
            glib_sys::g_main_context_unref(self.inner);
        }
    }
}

pub struct LockedMainContext {
    inner: MainContext,
    _marker: marker::PhantomData<Rc<()>>, // cannot share across threads
}

impl LockedMainContext {
    /// Runs a single iteration for the given main loop.
    ///
    /// This involves checking to see if any event sources are ready to be
    /// processed, then if no events sources are ready and may_block is `true`,
    /// waiting for a source to become ready, then dispatching the highest
    /// priority events sources that are ready. Otherwise, if may_block is
    /// `false` sources are not waited to become ready, only those highest
    /// priority events sources will be dispatched (if any), that are ready at
    /// this given moment without further waiting.
    ///
    /// Note that even when may_block is `true`, it is still possible for
    /// `iteration` to return `false`, since the wait may be interrupted for other
    /// reasons than an event source becoming ready.
    ///
    /// Returns `true` if events were dispatched.
    pub fn iteration(&self, may_block: bool) -> bool {
        let r = unsafe {
            glib_sys::g_main_context_iteration(self.inner.inner, may_block as c_int)
        };
        r == TRUE
    }

    /// Checks if any sources have pending events for the given context.
    pub fn pending(&self) -> bool {
        unsafe {
            glib_sys::g_main_context_pending(self.inner.inner) == TRUE
        }
    }

    /// Unlocks this context, returning the underlying unlocked main context.
    ///
    /// Note that this is not required to be called, if this context goes out of
    /// scope it will also be unlocked.
    pub fn unlock(self) -> MainContext {
        // TODO: avoid frobbing the refcount here.
        self.inner.clone()
    }
}

impl Drop for LockedMainContext {
    fn drop(&mut self) {
        unsafe {
            glib_sys::g_main_context_release(self.inner.inner);
        }
    }
}

/// An RAII struct that is returned from `push_thread_default` to pop the
/// default when it goes out of scope.
pub struct PushThreadDefault<'a> {
    inner: &'a MainContext,
}

impl<'a> Drop for PushThreadDefault<'a> {
    fn drop(&mut self) {
        unsafe {
            glib_sys::g_main_context_pop_thread_default(self.inner.inner);
        }
    }
}

pub struct MainLoop {
    inner: *mut glib_sys::GMainLoop, // TODO: send/sync ?
}

impl MainLoop {
    /// Creates a new event loop using the provided context.
    ///
    /// If `None` is provided then the default context will be used.
    pub fn new(cx: Option<&MainContext>) -> MainLoop {
        let cx = cx.map(|c| c.inner).unwrap_or(0 as *mut _);
        let ptr = unsafe {
            glib_sys::g_main_loop_new(cx, FALSE)
        };
        assert!(!ptr.is_null());
        MainLoop { inner: ptr }
    }

    /// Runs a main loop until `quit` is called on the loop.
    ///
    /// If this is called for the thread of the loop's `MainContext`, it will
    /// process events from the loop, otherwise it will simply wait.
    pub fn run(&self) {
        unsafe {
            glib_sys::g_main_loop_run(self.inner)
        }
    }

    /// Stops a `MainLoop` from running. Any calls to `run` for the
    /// loop will return.
    ///
    /// Note that sources that have already been dispatched when
    /// `quit` is called will still be executed.
    pub fn quit(&self) {
        unsafe {
            glib_sys::g_main_loop_quit(self.inner)
        }
    }

    /// Checks to see if the main loop is currently being run via
    /// `run`.
    pub fn is_running(&self) -> bool {
        unsafe {
            glib_sys::g_main_loop_is_running(self.inner) == TRUE
        }
    }

    /// Returns the context of this loop.
    pub fn context(&self) -> MainContext {
        unsafe {
            let ptr = glib_sys::g_main_loop_get_context(self.inner);
            glib_sys::g_main_context_ref(ptr);
            MainContext { inner: ptr }
        }
    }
}

impl Clone for MainLoop {
    fn clone(&self) -> MainLoop {
        unsafe {
            let ptr = glib_sys::g_main_loop_ref(self.inner);
            assert!(!ptr.is_null());
            MainLoop { inner: ptr }
        }
    }
}

impl Drop for MainLoop {
    fn drop(&mut self) {
        unsafe {
            glib_sys::g_main_loop_unref(self.inner);
        }
    }
}

/// A binding to the `GSource` underlying type.
pub struct Source<T> {
    inner: *mut glib_sys::GSource, // TODO: send/sync?
    _marker: marker::PhantomData<T>,
}

struct Inner<T> {
    _gsource: glib_sys::GSource,
    funcs: Box<glib_sys::GSourceFuncs>,
    data: T,
}

impl<T: SourceFuncs> Source<T> {
    /// Creates a new `GSource` structure.
    ///
    /// The source will not initially be associated with any `MainContext` and
    /// must be added to one with `attach` before it will be executed.
    pub fn new(t: T) -> Source<T> {
        unsafe {
            let size = mem::size_of::<Inner<T>>();
            assert!(size < <c_uint>::max_value() as usize);
            let mut funcs: Box<glib_sys::GSourceFuncs> = Box::new(mem::zeroed());
            funcs.prepare = Some(prepare::<T>);
            funcs.check = Some(check::<T>);
            funcs.dispatch = Some(dispatch::<T>);
            funcs.finalize = Some(finalize::<T>);
            let ptr = glib_sys::g_source_new(&mut *funcs, size as c_uint);
            assert!(!ptr.is_null());
            ptr::write(&mut (*(ptr as *mut Inner<T>)).data, t);
            ptr::write(&mut (*(ptr as *mut Inner<T>)).funcs, funcs);
            Source {
                inner: ptr,
                _marker: marker::PhantomData,
            }
        }
    }

    /// Acquires an underlying reference to the data contained within this
    /// `Source`.
    pub fn get_ref(&self) -> &T {
        unsafe { &( *(self.inner as *const Inner<T>) ).data }
    }

    /// Adds a `Source` to a context so that it will be executed within that
    /// context.
    pub fn attach(&self, context: &MainContext) -> c_uint {
        unsafe { glib_sys::g_source_attach(self.inner, context.inner) }
    }

    /// Sets the priority of a source.
    ///
    /// While the main loop is being run, a source will be dispatched if it is
    /// ready to be dispatched and no sources at a higher (numerically smaller)
    /// priority are ready to be dispatched.
    ///
    /// A child source always has the same priority as its parent. It is not
    /// permitted to change the priority of a source once it has been added as a
    /// child of another source.
    pub fn set_priority(&self, priority: i32) {
        unsafe { glib_sys::g_source_set_priority(self.inner, priority) }
    }

    /// Gets the priority of a source.
    pub fn priority(&self) -> i32 {
        unsafe { glib_sys::g_source_get_priority(self.inner) }
    }

    /// Sets whether a source can be called recursively.
    ///
    /// If can_recurse is `true`, then while the source is being dispatched then
    /// this source will be processed normally. Otherwise, all processing of
    /// this source is blocked until the dispatch function returns.
    pub fn set_can_recurse(&self, can_recurse: bool) {
        let can_recurse = if can_recurse { TRUE } else { FALSE };
        unsafe { glib_sys::g_source_set_can_recurse(self.inner, can_recurse) }
    }

    /// Checks whether a source is allowed to be called recursively.
    pub fn can_recurse(&self) -> bool {
        unsafe { glib_sys::g_source_get_can_recurse(self.inner) == TRUE }
    }

    /// Returns the numeric ID for a particular source.
    ///
    /// The ID of a source is a positive integer which is unique within a
    /// particular main loop context.
    pub fn get_id(&self) -> u32 {
        unsafe { glib_sys::g_source_get_id(self.inner) }
    }

    /// Gets the `MainContext` with which the source is associated.
    pub fn context(&self) -> Option<MainContext> {
        unsafe {
            let context = glib_sys::g_source_get_context(self.inner);
            if context.is_null() {
                None
            }
            else {
                glib_sys::g_main_context_ref(context);
                Some(MainContext {
                    inner: context,
                })
            }
        }
    }

    /// Sets a `Source` to be dispatched when the given monotonic time is
    /// reached (or passed). If the monotonic time is in the past (as it always
    /// will be if ready_time is the current time) then the source will be
    /// dispatched immediately.
    ///
    /// If ready_time is `None` then the source is never woken up on the basis
    /// of the passage of time.
    ///
    /// Dispatching the source does not reset the ready time. You should do so
    /// yourself, from the source dispatch function.
    ///
    /// Note that if you have a pair of sources where the ready time of one
    /// suggests that it will be delivered first but the priority for the other
    /// suggests that it would be delivered first, and the ready time for both
    /// sources is reached during the same main context iteration then the order
    /// of dispatch is undefined.
    ///
    /// This API is only intended to be used by implementations of `Source`. Do
    /// not call this API on a `Source` that you did not create.
    pub fn set_ready_time(&self, ready_time: Option<Instant>) {
        let time = match ready_time {
            Some(time) => {
                let now = Instant::now();
                if now < time {
                    let duration = time.duration_since(now);
                    let mono_time = unsafe { glib_sys::g_get_monotonic_time() };
                    (utils::millis(duration) * 1000) as i64 + mono_time
                }
                else {
                    0
                }
            },
            None => -1,
        };
        unsafe { glib_sys::g_source_set_ready_time(self.inner, time) }
    }
}

impl<T> Clone for Source<T> {
    fn clone(&self) -> Source<T> {
        unsafe {
            let ptr = glib_sys::g_source_ref(self.inner);
            assert!(!ptr.is_null());
            Source {
                inner: ptr,
                _marker: marker::PhantomData,
            }
        }
    }
}

impl<T> Drop for Source<T> {
    fn drop(&mut self) {
        unsafe {
            glib_sys::g_source_unref(self.inner);
        }
    }
}

/// Trait for the callbacks that will be invoked by the `Source` type.
pub trait SourceFuncs: Sized {
    /// Called before all the file descriptors are polled.
    ///
    /// If the source can determine that it is ready here (without waiting for
    /// the results of the poll() call) it should return `true`. It can also
    /// return a timeout value which should be the maximum timeout which should
    /// be passed to the poll() call.
    ///
    /// The actual timeout used will be -1 if all sources returned `None` or it
    /// will be the minimum of all the timeout_ values returned which were >= 0.
    /// If prepare returns a timeout and the source also has a 'ready time' set
    /// then the nearer of the two will be used.
    fn prepare(&self, source: &Source<Self>) -> (bool, Option<Duration>);

    /// Called after all the file descriptors are polled.
    ///
    /// The source should return `true` if it is ready to be dispatched. Note
    /// that some time may have passed since the previous prepare function was
    /// called, so the source should be checked again here.
    fn check(&self, source: &Source<Self>) -> bool;

    /// Called to dispatch the event source, after it has returned `true` in
    /// either its `prepare` or its `check` function.
    ///
    /// The dispatch function is passed in a callback function. The dispatch
    /// function should call the callback function. The return value of the
    /// dispatch function should be `false` if the source should be removed or
    /// `true` to keep it.
    fn dispatch<F: FnMut() -> bool>(&self, source: &Source<Self>, callback: F) -> bool;
}

unsafe extern fn prepare<T: SourceFuncs>(source: *mut glib_sys::GSource,
                                         timeout: *mut c_int)
                                         -> glib_sys::gboolean {
    let inner = source as *mut Inner<T>;
    let source = ManuallyDrop::new(Source {
        inner: source,
        _marker: marker::PhantomData,
    });
    let (ret, duration) = (*inner).data.prepare(source.get_ref());
    if !ret {
        return FALSE
    }
    if let Some(dur) = duration {
        *timeout = cmp::max(utils::millis(dur), <c_int>::max_value() as u64) as c_int;
    }
    return TRUE
}

unsafe extern fn check<T: SourceFuncs>(source: *mut glib_sys::GSource) -> glib_sys::gboolean {
    let inner = source as *mut Inner<T>;
    let source = ManuallyDrop::new(Source {
        inner: source,
        _marker: marker::PhantomData,
    });
    if (*inner).data.check(source.get_ref()) {
        1
    }
    else {
        0
    }
}

unsafe extern fn dispatch<T: SourceFuncs>(source: *mut glib_sys::GSource, source_func: glib_sys::GSourceFunc, data: glib_sys::gpointer) -> glib_sys::gboolean {
    let inner = source as *mut Inner<T>;
    let source = ManuallyDrop::new(Source {
        inner: source,
        _marker: marker::PhantomData,
    });
    if (*inner).data.dispatch(source.get_ref(), || {
        if let Some(source_func) = source_func {
            source_func(data) != 0
        }
        else {
            true
        }
    }) {
        1
    }
    else {
        0
    }
}

unsafe extern fn finalize<T: SourceFuncs>(source: *mut glib_sys::GSource) {
    let source = source as *mut Inner<T>;
    ptr::read(&(*source).funcs);
    ptr::read(&(*source).data);
}
