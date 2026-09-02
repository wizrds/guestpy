//! RustPython engine integration.

use std::{
    cell::{Cell, RefCell},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use guestpy_core::{
    backend::{Backend, NoNativeExtensions},
    errors::Error,
};

use rustpython_vm::{
    AsObject, Interpreter, PyObjectRef, Settings, VirtualMachine, builtins::PyDictRef,
    signal::UserSignalSender,
};

thread_local! {
    static DEFERRED: RefCell<Vec<PyObjectRef>> = const { RefCell::new(Vec::new()) };
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
    globals: PyDictRef,
    builtins: PyDictRef,
}

#[derive(Default)]
pub struct Config {
    pub settings: Settings,
}

pub struct Engine {
    interpreter: Interpreter,
    signals: UserSignalSender,
    interrupt: Arc<AtomicBool>,
    dispatcher_installed: Cell<bool>,
}

impl Engine {
    fn new(config: Config) -> Result<Self, Error> {
        let (signals, receiver) = rustpython_vm::signal::user_signal_channel();
        let builder = Interpreter::builder(config.settings)
            .init_hook(move |vm| vm.set_user_signal_channel(receiver));
        let native_defs = rustpython_stdlib::stdlib_module_defs(&builder.ctx);

        Ok(Self {
            interpreter: builder
                .add_native_modules(&native_defs)
                .add_frozen_modules(rustpython_pylib::FROZEN_STDLIB)
                .build(),
            signals,
            interrupt: Arc::new(AtomicBool::new(false)),
            dispatcher_installed: Cell::new(false),
        })
    }

    pub(crate) fn signals(&self) -> &UserSignalSender {
        &self.signals
    }

    pub(crate) fn interrupt(&self) -> &Arc<AtomicBool> {
        &self.interrupt
    }

    pub(crate) fn claim_dispatcher(&self) -> bool {
        self.dispatcher_installed.replace(true)
    }
}

pub struct RustPython;

impl Backend for RustPython {
    type Engine = Engine;
    type Context = Context;
    type Token<'py> = &'py VirtualMachine;
    type Value<'py> = PyObjectRef;
    type Owned = PyObjectRef;
    type Config = Config;
    type NativeExtensions = NoNativeExtensions;

    const NAME: &'static str = "rustpython";

    fn engine(config: Self::Config) -> Result<Self::Engine, Error> {
        Engine::new(config)
    }

    fn shutdown(engine: Self::Engine) -> Result<(), Error> {
        engine.interpreter.finalize(None);
        Ok(())
    }

    fn enter<F, R>(engine: &Self::Engine, f: F) -> R
    where
        F: for<'py> FnOnce(Self::Token<'py>) -> R,
    {
        let _interrupt = InterruptScope::new(engine.interrupt());

        engine.interpreter.enter(|vm| {
            drop(DEFERRED.with(|queue| std::mem::take(&mut *queue.borrow_mut())));
            f(vm)
        })
    }

    fn new_context<'py>(
        _: Self::Token<'py>,
        globals: Self::Value<'py>,
        builtins: Self::Value<'py>,
    ) -> Self::Context {
        Context {
            globals: globals.downcast().unwrap(),
            builtins: builtins.downcast().unwrap(),
        }
    }

    fn context_globals<'py>(_: Self::Token<'py>, context: &Self::Context) -> Self::Value<'py> {
        context.globals.clone().into()
    }

    fn context_builtins<'py>(_: Self::Token<'py>, context: &Self::Context) -> Self::Value<'py> {
        context.builtins.clone().into()
    }

    fn detach<'py>(_: Self::Token<'py>, value: Self::Value<'py>) -> Self::Owned {
        value
    }

    fn attach<'py>(_: Self::Token<'py>, owned: &Self::Owned) -> Self::Value<'py> {
        owned.clone()
    }

    fn owned_ptr_eq(first: &Self::Owned, second: &Self::Owned) -> bool {
        first.is(second)
    }

    fn release(owned: Self::Owned) {
        if rustpython_vm::vm::thread::current_vm_is_set() {
            drop(owned);
        } else {
            DEFERRED.with(|queue| queue.borrow_mut().push(owned));
        }
    }
}

#[cfg(test)]
mod tests {
    use guestpy_core::{
        backend::{Backend, BackendCallables, BackendClasses, BackendModules, BackendValues},
        bundle::Bundle,
        driver::Progress,
        errors::Error,
        handle::{Coroutine, ObjectProtocol},
        host::{
            class::{ClassBuilder, HostClass, HostClassDefinition},
            module::ModuleSpec,
        },
        marshal::args::Args,
        runtime::Runtime,
        scope::Enter,
    };
    use std::{cell::Cell, rc::Rc};

    use super::{Config, RustPython};

    fn mixed_bundle() -> Bundle {
        Bundle::builder()
            .package("plugin", "")
            .module(
                "plugin.util",
                r#"
VALUE = 21
"#,
            )
            .data("plugin/_native.cpython-313-x86_64-linux-gnu.so", b"native-bytes".to_vec())
            .data("plugin/.libs/libdependency.so", b"dependency-bytes".to_vec())
            .build()
            .unwrap()
    }

    guestpy_core::backend::fixtures::tests!(RustPython);

    #[test]
    fn evaluates_python_through_the_backend() {
        let engine = RustPython::engine(Config::default()).unwrap();

        let value = RustPython::enter(&engine, |token| {
            let globals = RustPython::new_dict(token)?;
            let builtins = RustPython::copy_dict(token, &RustPython::builtins_dict(token)?)?;

            RustPython::set_item(
                token,
                &globals,
                RustPython::str(token, "__builtins__"),
                builtins,
            )?;

            RustPython::exec(token, "def double(n):\n    return n * 2\n", "<test>", &globals)?;

            let result = RustPython::eval(token, "double(21)", "<test>", &globals)?;

            RustPython::as_i64(token, &result)
        })
        .unwrap();

        assert_eq!(value, 42);
    }

    #[test]
    fn binds_a_host_module() {
        let runtime = Runtime::<RustPython>::builder()
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

    #[tokio::test]
    async fn close_runs_finally_blocks() {
        let ran = Rc::new(Cell::new(false));
        let runtime = Runtime::<RustPython>::builder()
            .bind(ModuleSpec::new("host").function("mark", {
                let ran = ran.clone();

                move |_, _| {
                    ran.set(true);

                    Ok::<_, Error>(())
                }
            }))
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        guest
            .exec(concat!(
                "import asyncio, host\n",
                "async def work():\n",
                "    try:\n",
                "        await asyncio.sleep(3600)\n",
                "    finally:\n",
                "        host.mark()\n",
                "async def run():\n",
                "    asyncio.create_task(work())\n",
            ))
            .unwrap();

        guest
            .eval::<Coroutine<RustPython, ()>>("run()")
            .unwrap()
            .await
            .unwrap();
        for _ in 0..8 {
            if matches!(guest.advance().unwrap(), Progress::Waiting(_)) {
                break;
            }
        }

        assert!(!ran.get());

        assert!(matches!(guest.close(), Err(Error::Unexpected { .. })));

        guest
            .async_driver()
            .expect("async driver initialized")
            .close()
            .await
            .unwrap();
        guest.close().unwrap();

        assert!(ran.get());
    }

    #[test]
    fn close_is_idempotent() {
        let runtime = Runtime::<RustPython>::builder()
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        guest.exec("value = 1").unwrap();

        guest.close().unwrap();
        guest.close().unwrap();

        assert!(guest.is_closed());
        assert!(matches!(guest.exec("value = 2"), Err(Error::Closed)));
        assert!(matches!(guest.eval::<i64>("value"), Err(Error::Closed)));
    }

    #[tokio::test]
    async fn drop_without_close_is_safe() {
        let runtime = Runtime::<RustPython>::builder()
            .build()
            .unwrap();

        {
            let guest = runtime.guest().build().unwrap();

            guest
                .exec(concat!(
                    "import asyncio\n",
                    "async def work():\n",
                    "    await asyncio.sleep(3600)\n",
                    "async def run():\n",
                    "    asyncio.create_task(work())\n",
                ))
                .unwrap();

            guest
                .eval::<Coroutine<RustPython, ()>>("run()")
                .unwrap()
                .await
                .unwrap();
        }

        let fresh = runtime.guest().build().unwrap();

        assert_eq!(fresh.eval::<i64>("6 * 7").unwrap(), 42);
    }

    #[tokio::test]
    async fn payloads_drop_exactly_once() {
        thread_local! {
            static DROPS: Cell<usize> = const { Cell::new(0) };
        }

        struct Payload;

        impl Drop for Payload {
            fn drop(&mut self) {
                DROPS.with(|drops| drops.set(drops.get() + 1));
            }
        }

        impl HostClass for Payload {
            const NAME: &'static str = "Payload";
        }

        impl<B> HostClassDefinition<B> for Payload
        where
            B: Backend + BackendValues + BackendCallables + BackendClasses,
        {
            fn construct<'py>(_: &Enter<'py, B>, _: Args<'py, B>) -> Result<Self, Error> {
                Ok(Self)
            }

            fn build(_: &mut ClassBuilder<B, Self>) {}
        }

        DROPS.with(|drops| drops.set(0));

        let runtime = Runtime::<RustPython>::builder()
            .bind(ModuleSpec::new("host").class::<Payload>())
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        guest
            .exec("import host\nitems = [host.Payload() for _ in range(5)]")
            .unwrap();

        guest.close().unwrap();

        assert_eq!(DROPS.with(|drops| drops.get()), 5);
    }

    #[test]
    fn shutdown_fails_with_a_live_guest() {
        let runtime = Runtime::<RustPython>::builder()
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();
        let retained = runtime.clone();

        assert!(matches!(runtime.shutdown(), Err(Error::Unexpected { .. })));

        drop(guest);

        retained.shutdown().unwrap();
    }

    #[test]
    fn dropping_the_runtime_first_is_legal() {
        let runtime = Runtime::<RustPython>::builder()
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        drop(runtime);

        assert_eq!(guest.eval::<i64>("6 * 7").unwrap(), 42);

        drop(guest);
    }

    #[test]
    fn imports_pure_python_from_a_mixed_bundle() {
        let runtime = Runtime::<RustPython>::builder()
            .bundle(mixed_bundle())
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        guest
            .exec("import plugin.util")
            .unwrap();

        assert_eq!(
            guest
                .eval::<i64>("plugin.util.VALUE * 2")
                .unwrap(),
            42
        );
    }

    #[test]
    fn native_imports_report_the_backend_as_unsupported() {
        let runtime = Runtime::<RustPython>::builder()
            .bundle(mixed_bundle())
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        let error = match guest.exec("import plugin._native") {
            Err(Error::Guest(exception)) => exception,
            other => panic!("expected a guest exception, got: {other:?}"),
        };

        assert!(error.matches("NotImplementedError"));
        assert!(error.message().contains("rustpython"));
        assert!(
            error
                .message()
                .contains("plugin._native")
        );
    }

    #[test]
    fn unused_native_dependencies_do_not_block_guest_creation() {
        let runtime = Runtime::<RustPython>::builder()
            .bundle(mixed_bundle())
            .build()
            .unwrap();

        assert!(runtime.guest().build().is_ok());
    }
}
