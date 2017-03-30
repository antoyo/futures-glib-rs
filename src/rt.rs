use std::sync::atomic::{AtomicBool, Ordering, ATOMIC_BOOL_INIT};
use std::cell::Cell;

thread_local! {
    static IS_MAIN_THREAD: Cell<bool> = Cell::new(false)
}

static INITIALIZED: AtomicBool = ATOMIC_BOOL_INIT;

/// Asserts that this is the main thread and `futures_glib::init` has been called.
macro_rules! assert_initialized_main_thread {
    () => (
        if !::rt::is_initialized_main_thread() {
            if ::rt::is_initialized() {
                panic!("Glib may only be used from the main thread.");
            }
            else {
                panic!("Glib has not been initialized. Call `futures_glib::init` first.");
            }
        }
    )
}

/// No-op.
macro_rules! skip_assert_initialized {
    () => ()
}

/// Returns `true` if Glib has been initialized and this is the main thread.
#[inline]
pub fn is_initialized_main_thread() -> bool {
    skip_assert_initialized!();
    IS_MAIN_THREAD.with(|c| c.get())
}

/// Returns `true` if Glib has been initialized.
#[inline]
pub fn is_initialized() -> bool {
    skip_assert_initialized!();
    INITIALIZED.load(Ordering::Acquire)
}

/// Informs this crate that Glib has been initialized and the current thread is the main one.
pub unsafe fn set_initialized() {
    skip_assert_initialized!();
    if is_initialized_main_thread() {
        return;
    }
    else if is_initialized() {
        panic!("Attempted to initialize Glib from two different threads.");
    }
    INITIALIZED.store(true, Ordering::Release);
    IS_MAIN_THREAD.with(|c| c.set(true));
}

/// Tries to initialize Glib.
pub fn init() {
    skip_assert_initialized!();
    if is_initialized_main_thread() {
        return;
    }
    else if is_initialized() {
        panic!("Attempted to initialize Glib from two different threads.");
    }
    unsafe {
        set_initialized();
    }
}
