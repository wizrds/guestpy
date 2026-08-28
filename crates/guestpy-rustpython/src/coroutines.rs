//! RustPython coroutine operations.

use guestpy_core::{
    backend::{BackendCoroutines, Tok, Val},
    errors::Error,
};
use rustpython_vm::{AsObject, builtins::PyCoroutine};

use crate::{engine::RustPython, errors::NativeErrors};

impl BackendCoroutines for RustPython {
    fn is_coroutine<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value.downcastable::<PyCoroutine>()
    }

    fn is_awaitable<'py>(vm: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value.downcastable::<PyCoroutine>()
            || value
                .class()
                .get_attr(vm.ctx.intern_str("__await__"))
                .is_some()
    }

    fn anext<'py>(
        vm: Tok<'py, Self>,
        async_iterator: &Val<'py, Self>,
    ) -> Result<Val<'py, Self>, Error> {
        vm.call_method(async_iterator, "__anext__", ())
            .map_err(|error| RustPython::guest(vm, error))
    }

    fn asend<'py>(
        vm: Tok<'py, Self>,
        async_generator: &Val<'py, Self>,
        value: Val<'py, Self>,
    ) -> Result<Val<'py, Self>, Error> {
        vm.call_method(async_generator, "asend", (value,))
            .map_err(|error| RustPython::guest(vm, error))
    }

    fn athrow<'py>(
        vm: Tok<'py, Self>,
        async_generator: &Val<'py, Self>,
        exception: Val<'py, Self>,
    ) -> Result<Val<'py, Self>, Error> {
        vm.call_method(async_generator, "athrow", (exception,))
            .map_err(|error| RustPython::guest(vm, error))
    }

    fn aclose<'py>(
        vm: Tok<'py, Self>,
        async_generator: &Val<'py, Self>,
    ) -> Result<Val<'py, Self>, Error> {
        vm.call_method(async_generator, "aclose", ())
            .map_err(|error| RustPython::guest(vm, error))
    }

    fn set_running_loop<'py>(
        vm: Tok<'py, Self>,
        asyncio_loop: Option<&Val<'py, Self>>,
    ) -> Result<(), Error> {
        *vm.asyncio_running_loop.borrow_mut() = asyncio_loop.cloned();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc, time::Duration};

    use guestpy_core::{
        backend::{
            Backend, BackendCallables, BackendModules, BackendValues, Step, callables::RawBody,
        },
        driver::Progress,
        errors::Error,
        handle::Coroutine,
        host::module::ModuleSpec,
        policy::Cancellation,
        runtime::Runtime,
    };
    use rustpython_vm::PyObjectRef;
    use tokio::task::LocalSet;

    use crate::engine::{Config, Engine, RustPython};

    guestpy_core::backend::coroutines::fixtures::tests!(RustPython);

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

        fn exec(&self, source: &str) {
            RustPython::enter(&self.engine, |vm| {
                RustPython::exec(vm, source, "<test>", &self.globals)
            })
            .unwrap();
        }

        fn eval(&self, source: &str) -> PyObjectRef {
            RustPython::enter(&self.engine, |vm| {
                RustPython::eval(vm, source, "<test>", &self.globals)
            })
            .unwrap()
        }

        fn function(&self, name: &str, body: RawBody<RustPython>) {
            RustPython::enter(&self.engine, |vm| {
                RustPython::set_item(
                    vm,
                    &self.globals,
                    RustPython::str(vm, name),
                    RustPython::function(vm, name, None, body)?,
                )
            })
            .unwrap();
        }

        fn send(&self, coroutine: &PyObjectRef) -> Step<PyObjectRef> {
            RustPython::enter(&self.engine, |vm| RustPython::send(vm, coroutine, vm.ctx.none()))
                .unwrap()
        }

        fn throw(&self, coroutine: &PyObjectRef, exception: PyObjectRef) -> Step<PyObjectRef> {
            RustPython::enter(&self.engine, |vm| RustPython::throw(vm, coroutine, exception))
                .unwrap()
        }

        fn close(&self, coroutine: &PyObjectRef) {
            RustPython::enter(&self.engine, |vm| RustPython::close(vm, coroutine)).unwrap();
        }

        fn as_i64(&self, value: &PyObjectRef) -> i64 {
            RustPython::enter(&self.engine, |vm| RustPython::as_i64(vm, value)).unwrap()
        }

        fn as_str(&self, value: &PyObjectRef) -> String {
            RustPython::enter(&self.engine, |vm| RustPython::as_str(vm, value)).unwrap()
        }
    }

    #[test]
    fn steps_a_coroutine_to_completion() {
        let fixture = Fixture::new();

        fixture.exec("async def run():\n    return 7");

        match fixture.send(&fixture.eval("run()")) {
            Step::Yielded(_) => panic!("coroutine yielded instead of returning"),
            Step::Returned(value) => assert_eq!(fixture.as_i64(&value), 7),
        }
    }

    #[test]
    fn yields_then_returns() {
        let fixture = Fixture::new();

        fixture.exec(
            "class Once:\n    def __await__(self):\n        yield 'waiting'\n        return 7\n\nasync def run():\n    return await Once()",
        );

        let coroutine = fixture.eval("run()");

        match fixture.send(&coroutine) {
            Step::Yielded(value) => assert_eq!(fixture.as_str(&value), "waiting"),
            Step::Returned(_) => panic!("coroutine returned instead of yielding"),
        }

        match fixture.send(&coroutine) {
            Step::Yielded(_) => panic!("coroutine yielded instead of returning"),
            Step::Returned(value) => assert_eq!(fixture.as_i64(&value), 7),
        }
    }

    #[test]
    fn throw_delivers_an_exception_into_the_frame() {
        let fixture = Fixture::new();

        fixture.exec(
            "class Wait:\n    def __await__(self):\n        yield None\n\nclass Marker(BaseException):\n    pass\n\nasync def run():\n    try:\n        await Wait()\n    except BaseException as error:\n        return error\n\nmarker = Marker()",
        );

        let coroutine = fixture.eval("run()");
        let exception = fixture.eval("marker");

        assert!(matches!(fixture.send(&coroutine), Step::Yielded(_)));

        match fixture.throw(&coroutine, exception.clone()) {
            Step::Yielded(_) => panic!("coroutine yielded instead of returning"),
            Step::Returned(returned) => {
                assert!(RustPython::owned_ptr_eq(&returned, &exception));
            }
        }
    }

    #[test]
    fn close_runs_finally() {
        let fixture = Fixture::new();
        let called = Rc::new(Cell::new(false));

        fixture.function("mark", {
            let called = called.clone();

            Rc::new(move |call| {
                called.set(true);

                Ok(call.token.ctx.none())
            })
        });
        fixture.exec(
            "class Wait:\n    def __await__(self):\n        yield None\n\nasync def run():\n    try:\n        await Wait()\n    finally:\n        mark()",
        );

        let coroutine = fixture.eval("run()");

        assert!(matches!(fixture.send(&coroutine), Step::Yielded(_)));

        fixture.close(&coroutine);

        assert!(called.get());
    }

    #[tokio::test]
    async fn awaits_a_guest_coroutine() {
        let runtime = Runtime::<RustPython>::builder()
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        guest
            .exec("async def run():\n    return 21 * 2")
            .unwrap();

        let value = guest
            .eval::<Coroutine<RustPython, i64>>("run()")
            .unwrap()
            .await
            .unwrap();

        assert_eq!(value, 42);
    }

    #[tokio::test]
    async fn a_guest_awaits_a_host_async_function() {
        let runtime = Runtime::<RustPython>::builder()
            .bind(ModuleSpec::new("host").async_function("double", |enter, args| {
                let n = args.required::<i64>(enter, 0, "n")?;

                Ok(async move { Ok::<_, Error>(n * 2) })
            }))
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        guest
            .exec(
                r#"
import host
"#,
            )
            .unwrap();

        let value = guest
            .eval::<Coroutine<RustPython, i64>>("host.double(21)")
            .unwrap()
            .await
            .unwrap();

        assert_eq!(value, 42);
    }

    #[tokio::test]
    async fn the_host_future_completes_out_of_order() {
        let runtime = Runtime::<RustPython>::builder()
            .bind(
                ModuleSpec::new("host")
                    .async_function("slow", |_enter, _args| {
                        Ok(async {
                            tokio::time::sleep(Duration::from_millis(30)).await;

                            Ok::<_, Error>(1_i64)
                        })
                    })
                    .async_function("fast", |_enter, _args| {
                        Ok(async {
                            tokio::time::sleep(Duration::from_millis(10)).await;

                            Ok::<_, Error>(2_i64)
                        })
                    }),
            )
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        guest
            .exec(concat!(
                "import asyncio, host\n",
                "async def run():\n",
                "    return await asyncio.gather(host.slow(), host.fast())\n",
            ))
            .unwrap();

        let values = guest
            .eval::<Coroutine<RustPython, Vec<i64>>>("run()")
            .unwrap()
            .await
            .unwrap();

        assert_eq!(values, vec![1, 2]);
    }

    #[tokio::test]
    async fn progress_reports_waiting() {
        let runtime = Runtime::<RustPython>::builder()
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        guest
            .exec(concat!(
                "import asyncio\n",
                "async def nap():\n",
                "    await asyncio.sleep(0.05)\n",
                "async def run():\n",
                "    asyncio.create_task(nap())\n",
            ))
            .unwrap();

        guest
            .eval::<Coroutine<RustPython, ()>>("run()")
            .unwrap()
            .await
            .unwrap();

        let mut waited = None;
        for _ in 0..8 {
            match guest.advance().unwrap() {
                Progress::Waiting(delay) => {
                    waited = Some(delay);

                    break;
                }
                Progress::Ready => continue,
                _ => panic!("expected Waiting or Ready"),
            }
        }
        assert!(waited.is_some_and(|delay| delay <= Duration::from_millis(50)));

        tokio::time::sleep(Duration::from_millis(60)).await;

        let mut idled = false;
        for _ in 0..8 {
            if matches!(guest.advance().unwrap(), Progress::Idle) {
                idled = true;

                break;
            }
        }
        assert!(idled);
    }

    #[tokio::test]
    async fn progress_reports_blocked() {
        let runtime = Runtime::<RustPython>::builder()
            .bind(ModuleSpec::new("host").async_function("hang", |_enter, _args| {
                Ok(async {
                    std::future::pending::<()>().await;

                    Ok::<_, Error>(())
                })
            }))
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        guest
            .exec(concat!(
                "import asyncio, host\n",
                "async def waiter():\n",
                "    await host.hang()\n",
                "async def run():\n",
                "    asyncio.create_task(waiter())\n",
            ))
            .unwrap();

        guest
            .eval::<Coroutine<RustPython, ()>>("run()")
            .unwrap()
            .await
            .unwrap();

        let mut blocked = false;
        for _ in 0..8 {
            match guest.advance().unwrap() {
                Progress::Blocked => {
                    blocked = true;

                    break;
                }
                Progress::Ready => continue,
                _ => panic!("expected Blocked or Ready"),
            }
        }
        assert!(blocked);
    }

    #[tokio::test]
    async fn an_exception_inside_a_coroutine_propagates() {
        let runtime = Runtime::<RustPython>::builder()
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        guest
            .exec("async def run():\n    raise ValueError('boom')")
            .unwrap();

        let error = guest
            .eval::<Coroutine<RustPython, ()>>("run()")
            .unwrap()
            .await
            .unwrap_err();

        assert!(matches!(error, Error::Guest(_)));
    }

    #[tokio::test]
    async fn a_loose_task_is_still_driven() {
        let runtime = Runtime::<RustPython>::builder()
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        guest
            .exec(concat!(
                "import asyncio\n",
                "done = []\n",
                "async def work():\n",
                "    await asyncio.sleep(0)\n",
                "    done.append(7)\n",
                "async def run():\n",
                "    asyncio.create_task(work())\n",
            ))
            .unwrap();

        guest
            .eval::<Coroutine<RustPython, ()>>("run()")
            .unwrap()
            .await
            .unwrap();
        guest.run_until_idle().await.unwrap();

        assert_eq!(guest.eval::<Vec<i64>>("done").unwrap(), vec![7]);
    }

    #[tokio::test]
    async fn nothing_reaches_the_real_event_loop_policy() {
        let runtime = Runtime::<RustPython>::builder()
            .build()
            .unwrap();
        let first = runtime.guest().build().unwrap();
        let second = runtime.guest().build().unwrap();

        for guest in [&first, &second] {
            guest
                .exec(concat!(
                    "import asyncio\n",
                    "async def loop_id():\n",
                    "    return id(asyncio.get_running_loop())\n",
                    "def has_running_loop():\n",
                    "    try:\n",
                    "        asyncio.get_running_loop()\n",
                    "        asyncio.get_running_loop()\n",
                    "        return True\n",
                    "    except RuntimeError:\n",
                    "        return False\n",
                ))
                .unwrap();
        }

        let a = first
            .eval::<Coroutine<RustPython, i64>>("loop_id()")
            .unwrap()
            .await
            .unwrap();
        let b = second
            .eval::<Coroutine<RustPython, i64>>("loop_id()")
            .unwrap()
            .await
            .unwrap();

        assert_ne!(a, b);

        assert!(
            !first
                .eval::<bool>("has_running_loop()")
                .unwrap()
        );
        assert!(
            !second
                .eval::<bool>("has_running_loop()")
                .unwrap()
        );
    }

    #[tokio::test]
    async fn interleaved_guests_keep_independent_deadlines() {
        let runtime = Runtime::<RustPython>::builder()
            .build()
            .unwrap();
        let brief = runtime
            .guest()
            .timeout(Duration::from_millis(30))
            .build()
            .unwrap();
        let ample = runtime
            .guest()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();

        brief
            .exec("import asyncio\nasync def spin():\n    while True:\n        await asyncio.sleep(0)")
            .unwrap();
        ample
            .exec("import asyncio\nasync def nap():\n    await asyncio.sleep(0.06)\n    return 7")
            .unwrap();

        LocalSet::new()
            .run_until(async {
                let brief_run = tokio::task::spawn_local({
                    let brief = brief.clone();

                    async move {
                        brief
                            .eval::<Coroutine<RustPython, ()>>("spin()")
                            .unwrap()
                            .await
                    }
                });
                let ample_run = tokio::task::spawn_local({
                    let ample = ample.clone();

                    async move {
                        ample
                            .eval::<Coroutine<RustPython, i64>>("nap()")
                            .unwrap()
                            .await
                    }
                });

                assert!(matches!(brief_run.await.unwrap(), Err(Error::Timeout)));
                assert_eq!(ample_run.await.unwrap().unwrap(), 7);
            })
            .await;
    }

    #[tokio::test]
    async fn a_timeout_stops_a_runaway_guest() {
        let runtime = Runtime::<RustPython>::builder()
            .timeout(Duration::from_millis(50))
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        guest
            .exec("import asyncio\nasync def spin():\n    while True:\n        await asyncio.sleep(0)")
            .unwrap();

        let result = guest
            .scope(async |scope| {
                scope
                    .eval::<Coroutine<RustPython, ()>>("spin()")?
                    .await
            })
            .await;

        assert!(matches!(result, Err(Error::Timeout)));
    }

    #[tokio::test]
    async fn a_cancellation_token_stops_a_runaway_guest() {
        let cancellation = Cancellation::new();
        let runtime = Runtime::<RustPython>::builder()
            .cancellation(cancellation.clone())
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        guest
            .exec("import asyncio\nasync def spin():\n    while True:\n        await asyncio.sleep(0)")
            .unwrap();

        LocalSet::new()
            .run_until(async {
                tokio::task::spawn_local({
                    let cancellation = cancellation.clone();

                    async move {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        cancellation.cancel();
                    }
                });

                let result = guest
                    .scope(async |scope| {
                        scope
                            .eval::<Coroutine<RustPython, ()>>("spin()")?
                            .await
                    })
                    .await;

                assert!(matches!(result, Err(Error::Cancelled)));
            })
            .await;
    }
}
