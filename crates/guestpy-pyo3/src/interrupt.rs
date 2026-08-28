use std::{
    ffi::c_void,
    os::raw::c_int,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use guestpy_core::{
    backend::{BackendInterrupt, Tok},
    errors::Error,
};
use pyo3::{PyErr, Python, exceptions::PyKeyboardInterrupt, ffi};

use crate::engine::{CPython, Engine, InterruptScope};

extern "C" fn trampoline(argument: *mut c_void) -> c_int {
    if !unsafe { Arc::from_raw(argument as *const AtomicBool) }.swap(false, Ordering::SeqCst) {
        return 0;
    }

    Python::attach(|py| {
        PyErr::new::<PyKeyboardInterrupt, _>("execution interrupted").restore(py);
    });

    -1
}

impl BackendInterrupt for CPython {
    type Handle = Arc<AtomicBool>;

    fn handle(engine: &Engine) -> Self::Handle {
        engine.interrupt().clone()
    }

    fn request(handle: &Self::Handle) {
        handle.store(true, Ordering::SeqCst);

        // Py_AddPendingCall's callback only runs on the main thread of the main
        // interpreter; a return of -1 here means the queue was full, but the flag is
        // set regardless, so the next `check` still trips.
        unsafe {
            ffi::Py_AddPendingCall(Some(trampoline), Arc::into_raw(handle.clone()) as *mut c_void);
        }
    }

    fn check<'py>(py: Tok<'py, Self>) -> Result<(), Error> {
        if InterruptScope::take() || py.check_signals().is_err() {
            return Err(Error::Interrupted);
        }

        Ok(())
    }

    fn reset(engine: &Engine) {
        engine
            .interrupt()
            .store(false, Ordering::SeqCst);
    }
}
