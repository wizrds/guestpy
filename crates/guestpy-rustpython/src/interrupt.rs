//! RustPython interruption operations.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use guestpy_core::{
    backend::{BackendInterrupt, Tok},
    errors::Error,
};
use rustpython_vm::{signal::UserSignalSender, vm::thread::current_vm_is_set};

use crate::engine::{Engine, InterruptScope, RustPython};

#[derive(Clone)]
pub struct InterruptHandle {
    sender: UserSignalSender,
    flag: Arc<AtomicBool>,
}

impl BackendInterrupt for RustPython {
    type Handle = InterruptHandle;

    fn handle(engine: &Engine) -> InterruptHandle {
        InterruptHandle {
            sender: engine.signals().clone(),
            flag: engine.interrupt().clone(),
        }
    }

    fn request(handle: &InterruptHandle) {
        handle
            .flag
            .store(true, Ordering::Release);

        if !current_vm_is_set() {
            let _ = handle.sender.send(Box::new(|vm| {
                Err(vm.new_exception_empty(
                    vm.ctx
                        .exceptions
                        .keyboard_interrupt
                        .to_owned(),
                ))
            }));
        }
    }

    fn check<'py>(vm: Tok<'py, Self>) -> Result<(), Error> {
        if InterruptScope::take() || vm.check_signals().is_err() {
            return Err(Error::Interrupted);
        }

        Ok(())
    }

    fn reset(engine: &Engine) {
        engine
            .interrupt()
            .store(false, Ordering::Release);
    }
}

// The two tight-loop tests below interrupt a non-yielding `while True: pass` running through the
// blocking `exec` path. The only lever for that is RustPython's process-global eval breaker, which
// a concurrent runtime can steal, so their delivery is irreducibly best-effort and they are
// serialized with TESTS_MUTEX. Making this fully deterministic would require a per-interpreter
// eval breaker upstream in RustPython itself.
#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
        thread,
        time::Duration,
    };

    use guestpy_core::{
        backend::{Backend, BackendInterrupt, BackendModules, BackendValues},
        errors::Error,
    };
    use rustpython_vm::PyObjectRef;

    use crate::engine::{Config, Engine, RustPython};

    static TESTS_MUTEX: Mutex<()> = Mutex::new(());

    struct Fixture {
        engine: Engine,
        globals: PyObjectRef,
    }

    impl Fixture {
        fn new() -> Self {
            let engine = RustPython::engine(Config::default()).unwrap();
            let globals = RustPython::enter(&engine, |vm| {
                let globals = RustPython::new_dict(vm)?;

                RustPython::set_item(
                    vm,
                    &globals,
                    RustPython::str(vm, "__builtins__"),
                    RustPython::builtins_dict(vm)?,
                )?;

                Ok::<_, Error>(globals)
            })
            .unwrap();

            Self { engine, globals }
        }

        fn exec(&self, source: &str) -> Result<(), Error> {
            RustPython::enter(&self.engine, |vm| {
                RustPython::exec(vm, source, "<test>", &self.globals)
            })
        }

        // Runs a non-yielding `source` and returns the error that stops it. RustPython's eval
        // breaker is a process-global word, so a single `request` can be cleared by any other VM's
        // `check_signals` before this tight loop observes it. The companion thread re-arms on a
        // short interval until `exec` returns, which makes delivery reliable here without altering
        // `request` or adding a production watchdog.
        fn exec_until_interrupted(&self, source: &str) -> Error {
            let handle = RustPython::handle(&self.engine);
            let finished = Arc::new(AtomicBool::new(false));
            let interrupt = thread::spawn({
                let finished = finished.clone();

                move || {
                    thread::sleep(Duration::from_millis(100));

                    while !finished.load(Ordering::Acquire) {
                        RustPython::request(&handle);

                        thread::sleep(Duration::from_millis(1));
                    }
                }
            });

            let error = self.exec(source).unwrap_err();
            finished.store(true, Ordering::Release);
            interrupt.join().unwrap();

            error
        }

        fn assert_keyboard_interrupt(error: Error) {
            match error {
                Error::Guest(exception) => assert!(exception.matches("KeyboardInterrupt")),
                other => panic!("expected KeyboardInterrupt, got: {other}"),
            }
        }
    }

    #[test]
    fn interrupts_a_tight_loop() {
        let _tests_guard = TESTS_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());

        let fixture = Fixture::new();

        Fixture::assert_keyboard_interrupt(fixture.exec_until_interrupted("while True: pass"));
    }

    #[test]
    fn interrupt_is_not_catchable_by_guest_code() {
        let _tests_guard = TESTS_MUTEX
            .lock()
            .unwrap_or_else(|error| error.into_inner());

        let fixture = Fixture::new();

        Fixture::assert_keyboard_interrupt(
            fixture
                .exec_until_interrupted("try:\n    while True: pass\nexcept Exception:\n    pass"),
        );
    }

    #[test]
    fn check_works_without_the_stdlib() {
        let fixture = Fixture::new();
        let handle = RustPython::handle(&fixture.engine);

        RustPython::request(&handle);

        assert!(matches!(
            RustPython::enter(&fixture.engine, RustPython::check),
            Err(Error::Interrupted),
        ));
    }
}
