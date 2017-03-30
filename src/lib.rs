extern crate futures;
extern crate glib_sys;
extern crate libc;
extern crate slab;

pub mod future;
mod stack;
mod timeout;

use std::marker;
use std::mem;
use std::ptr;
use std::time::Instant;

use libc::{c_int, c_uint};

pub use timeout::*;

const FALSE: c_int = 0;

struct MyDrop<T> { inner: Option<T> }

impl<T> MyDrop<T> {
    fn new(t: T) -> MyDrop<T> {
        MyDrop { inner: Some(t) }
    }

    fn get_ref(&self) -> &T {
        self.inner.as_ref().unwrap()
    }
}

impl<T> Drop for MyDrop<T> {
    fn drop(&mut self) {
        mem::forget(self.inner.take())
    }
}

pub struct MainContext {
    inner: *mut glib_sys::GMainContext, // TODO: send/sync?
}

unsafe impl Send for MainContext {}
unsafe impl Sync for MainContext {}

impl MainContext {
    pub fn new() -> MainContext {
        unsafe {
            let ptr = glib_sys::g_main_context_new();
            assert!(!ptr.is_null());
            MainContext { inner: ptr }
        }
    }

    pub fn default<F, R>(f: F) -> R
        where F: FnOnce(&MainContext) -> R
    {
        unsafe {
            let ptr = glib_sys::g_main_context_default();
            assert!(!ptr.is_null());
            let cx = MyDrop::new(MainContext { inner: ptr });
            f(cx.get_ref())

        }
    }

    /*pub fn turn(&self, may_block: bool) -> bool {
        let r = unsafe {
            glib_sys::g_main_context_iteration(self.inner, may_block as c_int)
        };
        r == TRUE
    }

    pub fn pending(&self) -> bool {
        unsafe {
            glib_sys::g_main_context_pending(self.inner) == TRUE
        }
    }*/

    pub fn wakeup(&self) {
        unsafe {
            glib_sys::g_main_context_wakeup(self.inner)
        }
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

pub struct MainLoop {
    inner: *mut glib_sys::GMainLoop,
}

impl MainLoop {
    pub fn new(cx: &MainContext) -> MainLoop {
        let ptr = unsafe {
            glib_sys::g_main_loop_new(cx.inner, FALSE)
        };
        assert!(!ptr.is_null());
        MainLoop { inner: ptr }
    }

    pub fn run(&self) {
        unsafe {
            glib_sys::g_main_loop_run(self.inner)
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

    pub fn get_ref(&self) -> &T {
        unsafe { &( *(self.inner as *const Inner<T>) ).data }
    }

    pub fn attach(&self, context: &MainContext) -> c_uint {
        unsafe { glib_sys::g_source_attach(self.inner, context.inner) }
    }

    pub fn get_context(&self) -> Option<MainContext> {
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

    pub fn set_ready_time(&self, ready_time: Option<Instant>) {
        let time = match ready_time {
            Some(time) => {
                let now = Instant::now();
                if now < time {
                    let duration = time.duration_since(now);
                    let mono_time = unsafe { glib_sys::g_get_monotonic_time() };
                    (timeout::millis(duration) * 1000) as i64 + mono_time
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

pub trait SourceFuncs: Sized {
    fn prepare(&self, source: &Source<Self>, timeout: &mut c_int) -> bool;
    fn check(&self, source: &Source<Self>) -> bool;
    fn dispatch<F: FnMut() -> bool>(&self, source: &Source<Self>, callback: F) -> bool;
}

unsafe extern fn prepare<T: SourceFuncs>(source: *mut glib_sys::GSource, timeout: *mut c_int) -> glib_sys::gboolean {
    let inner = source as *mut Inner<T>;
    let source = MyDrop::new(Source {
        inner: source,
        _marker: marker::PhantomData,
    });
    if (*inner).data.prepare(source.get_ref(), &mut *timeout) {
        1
    }
    else {
        0
    }
}

unsafe extern fn check<T: SourceFuncs>(source: *mut glib_sys::GSource) -> glib_sys::gboolean {
    let inner = source as *mut Inner<T>;
    let source = MyDrop::new(Source {
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
    let source = MyDrop::new(Source {
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
