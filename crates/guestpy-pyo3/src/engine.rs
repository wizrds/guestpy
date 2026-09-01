use std::{
    cell::RefCell,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use guestpy_core::{backend::Backend, errors::Error};
use pyo3::{Bound, Py, PyAny, Python};

use crate::{native_extensions::CPythonNativeExtensions, values::AsDict};

thread_local! {
    static ACTIVE_INTERRUPT: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
}

pub(crate) struct InterruptScope {
    previous: Option<Arc<AtomicBool>>,
}

impl InterruptScope {
    pub(crate) fn new(flag: &Arc<AtomicBool>) -> Self {
        Self {
            previous: ACTIVE_INTERRUPT.with(|slot| slot.borrow_mut().replace(flag.clone())),
        }
    }

    pub(crate) fn take() -> bool {
        ACTIVE_INTERRUPT.with(|slot| {
            slot.borrow()
                .as_ref()
                .is_some_and(|flag| flag.swap(false, Ordering::Acquire))
        })
    }
}

impl Drop for InterruptScope {
    fn drop(&mut self) {
        ACTIVE_INTERRUPT.with(|slot| *slot.borrow_mut() = self.previous.take());
    }
}

pub struct Context {
    globals: Object,
    builtins: Object,
}

pub struct Config {
    pub initialise: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self { initialise: true }
    }
}

pub struct Engine {
    interrupt: Arc<AtomicBool>,
}

impl Engine {
    fn new(config: Config) -> Result<Self, Error> {
        if config.initialise {
            Python::initialize();
        }

        Ok(Self {
            interrupt: Arc::new(AtomicBool::new(false)),
        })
    }

    pub(crate) fn interrupt(&self) -> &Arc<AtomicBool> {
        &self.interrupt
    }
}

pub struct Object(Option<Py<PyAny>>);

impl Object {
    pub(crate) fn new(value: Py<PyAny>) -> Self {
        Self(Some(value))
    }

    pub(crate) fn bind<'py>(&self, py: Python<'py>) -> Bound<'py, PyAny> {
        self.inner().bind(py).clone()
    }

    pub(crate) fn ptr_eq(&self, other: &Self) -> bool {
        self.inner().as_ptr() == other.inner().as_ptr()
    }

    fn inner(&self) -> &Py<PyAny> {
        self.0
            .as_ref()
            .expect("Object's inner value is only taken in Drop")
    }
}

impl Clone for Object {
    fn clone(&self) -> Self {
        Python::attach(|py| Self(Some(self.inner().clone_ref(py))))
    }
}

impl Drop for Object {
    fn drop(&mut self) {
        // `Py<T>`'s own `Drop` defers the decref to whichever thread next attaches if this
        // thread isn't attached right now, which can hand the drop of an `unsendable`
        // pyclass to a different OS thread than the one that created it. Attaching here
        // forces the decref to run immediately, on this thread, closing that gap.
        if let Some(value) = self.0.take() {
            Python::attach(|_| drop(value));
        }
    }
}

pub struct CPython;

impl Backend for CPython {
    type Engine = Engine;
    type Context = Context;
    type Token<'py> = Python<'py>;
    type Value<'py> = Bound<'py, PyAny>;
    type Owned = Object;
    type Config = Config;
    type NativeExtensions = CPythonNativeExtensions;

    const NAME: &'static str = "cpython";

    fn engine(config: Self::Config) -> Result<Self::Engine, Error> {
        Engine::new(config)
    }

    fn shutdown(_: Self::Engine) -> Result<(), Error> {
        Ok(())
    }

    fn enter<F, R>(engine: &Self::Engine, f: F) -> R
    where
        F: for<'py> FnOnce(Self::Token<'py>) -> R,
    {
        let _interrupt = InterruptScope::new(engine.interrupt());

        Python::attach(f)
    }

    fn new_context<'py>(
        _: Self::Token<'py>,
        globals: Self::Value<'py>,
        builtins: Self::Value<'py>,
    ) -> Self::Context {
        Context {
            globals: Object::new(
                globals
                    .as_dict()
                    .unwrap()
                    .unbind()
                    .into_any(),
            ),
            builtins: Object::new(
                builtins
                    .as_dict()
                    .unwrap()
                    .unbind()
                    .into_any(),
            ),
        }
    }

    fn context_globals<'py>(py: Self::Token<'py>, context: &Self::Context) -> Self::Value<'py> {
        context.globals.bind(py)
    }

    fn context_builtins<'py>(py: Self::Token<'py>, context: &Self::Context) -> Self::Value<'py> {
        context.builtins.bind(py)
    }

    fn detach<'py>(_: Self::Token<'py>, value: Self::Value<'py>) -> Self::Owned {
        Object::new(value.unbind())
    }

    fn attach<'py>(py: Self::Token<'py>, owned: &Self::Owned) -> Self::Value<'py> {
        owned.bind(py)
    }

    fn release(owned: Self::Owned) {
        drop(owned);
    }

    fn owned_ptr_eq(first: &Self::Owned, second: &Self::Owned) -> bool {
        first.ptr_eq(second)
    }
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use guestpy_core::{
        backend::{Backend, BackendInterrupt},
        errors::Error,
        handle::{ObjectProtocol, Value},
        host::module::ModuleSpec,
        runtime::Runtime,
    };
    use pyo3::Python;

    use super::{CPython, Config, Engine};

    guestpy_core::backend::fixtures::tests!(CPython);

    #[test]
    fn calls_a_host_module() {
        let runtime = Runtime::<CPython>::builder()
            .bind(ModuleSpec::new("m").function("double", |enter, args| {
                Ok::<_, Error>(args.required::<i64>(enter, 0, "n")? * 2)
            }))
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        assert_eq!(
            guest
                .host_module("m")
                .unwrap()
                .call_method::<_, i64>("double", (21,))
                .unwrap(),
            42,
        );
    }

    #[test]
    fn two_runtimes_share_one_interpreter() {
        let first = Runtime::<CPython>::builder()
            .build()
            .unwrap();
        let second = Runtime::<CPython>::builder()
            .build()
            .unwrap();
        let first_guest = first.guest().build().unwrap();
        let second_guest = second.guest().build().unwrap();

        first_guest.exec("import json").unwrap();
        second_guest
            .exec("import json")
            .unwrap();

        assert!(
            first_guest
                .eval::<bool>("json is __import__('json')")
                .unwrap(),
        );
        assert!(
            second_guest
                .eval::<bool>("json is __import__('json')")
                .unwrap(),
        );
    }

    #[test]
    fn shutdown_leaves_the_interpreter_running() {
        let runtime = Runtime::<CPython>::builder()
            .build()
            .unwrap();

        runtime.shutdown().unwrap();

        let runtime = Runtime::<CPython>::builder()
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        assert_eq!(guest.eval::<i64>("6 * 7").unwrap(), 42);
    }

    #[test]
    fn release_outside_an_enter_runs_del() {
        let runtime = Runtime::<CPython>::builder()
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        guest
            .exec(concat!(
                "dropped = []\n",
                "class Marks:\n",
                "    def __del__(self):\n",
                "        dropped.append(1)\n",
                "marker = Marks()\n",
            ))
            .unwrap();

        let value = guest
            .eval::<Value<CPython>>("marker")
            .unwrap();

        guest.exec("del marker").unwrap();

        drop(value);

        guest.exec("pass").unwrap();

        assert_eq!(
            guest
                .eval::<i64>("len(dropped)")
                .unwrap(),
            1
        );
    }

    #[test]
    fn initialise_false_uses_a_running_interpreter() {
        Python::initialize();

        let runtime = Runtime::<CPython>::builder()
            .config(Config { initialise: false })
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        assert_eq!(guest.eval::<i64>("6 * 7").unwrap(), 42);
    }

    #[test]
    fn interrupt_off_the_main_thread_lands_at_the_next_check() {
        let engine = Engine::new(Config::default()).unwrap();
        let handle = CPython::handle(&engine);

        thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));

            CPython::request(&handle);
        })
        .join()
        .unwrap();

        assert!(matches!(CPython::enter(&engine, CPython::check), Err(Error::Interrupted),));
    }
}
