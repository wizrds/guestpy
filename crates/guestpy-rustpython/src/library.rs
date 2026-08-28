use std::rc::Rc;

use guestpy_core::{
    backend::{BackendLibrary, Tok, Val},
    errors::Error,
};
use rustpython_vm::{PyObjectRef, VirtualMachine};

use crate::engine::RustPython;

impl BackendLibrary for RustPython {
    type NativeModule = Rc<dyn Fn(&VirtualMachine) -> Result<PyObjectRef, Error>>;

    fn declare_native<'py>(
        vm: Tok<'py, Self>,
        native: &Self::NativeModule,
        _: &str,
    ) -> Result<Val<'py, Self>, Error> {
        native(vm)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use guestpy_core::{
        bundle::Bundle,
        errors::{Error, GuestException},
        host::{
            library::{HostInitializer, HostLibrary},
            module::ModuleSpec,
        },
        native::{NativeInitializer, NativeLibrary, NativeModule},
        runtime::Runtime,
    };
    use rustpython_vm::{PyObjectRef, VirtualMachine};

    use crate::{engine::RustPython, errors::NativeErrors};

    struct Fixtures;

    impl Fixtures {
        fn native_module(
            name: &'static str,
            answer: i64,
        ) -> Rc<dyn Fn(&VirtualMachine) -> Result<PyObjectRef, Error>> {
            Rc::new(move |vm| {
                let dict = vm.ctx.new_dict();

                dict.set_item("answer", vm.ctx.new_int(answer).into(), vm)
                    .map_err(|error| RustPython::guest(vm, error))?;

                Ok(vm.new_module(name, dict, None).into())
            })
        }
    }

    struct Raises;

    impl Raises {
        fn guest(error: Error) -> GuestException {
            match error {
                Error::Guest(exception) => exception,
                other => panic!("expected a guest exception, got: {other}"),
            }
        }
    }

    #[test]
    fn binds_a_native_module_at_the_runtime_level() {
        let runtime = Runtime::<RustPython>::builder()
            .bind_native(NativeModule::new("fastmath", Fixtures::native_module("fastmath", 42)))
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        guest.exec("import fastmath").unwrap();

        assert_eq!(
            guest
                .eval::<i64>("fastmath.answer")
                .unwrap(),
            42
        );
    }

    #[test]
    fn binds_a_native_library_at_the_guest_level() {
        let runtime = Runtime::<RustPython>::builder()
            .build()
            .unwrap();
        let guest = runtime
            .guest()
            .bind_native(
                NativeLibrary::new()
                    .with(NativeModule::new("fastmath", Fixtures::native_module("fastmath", 7))),
            )
            .build()
            .unwrap();
        let other = runtime.guest().build().unwrap();

        guest.exec("import fastmath").unwrap();

        assert_eq!(
            guest
                .eval::<i64>("fastmath.answer")
                .unwrap(),
            7
        );
        assert!(
            Raises::guest(
                other
                    .exec("import fastmath")
                    .unwrap_err()
            )
            .matches("ImportError"),
        );
    }

    #[test]
    fn an_alias_imports_the_same_native_module() {
        let runtime = Runtime::<RustPython>::builder()
            .bind_native(
                NativeModule::new("fastmath", Fixtures::native_module("fastmath", 42))
                    .alias("fast"),
            )
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        guest.exec("import fast").unwrap();

        assert_eq!(
            guest
                .eval::<i64>("fast.answer")
                .unwrap(),
            42
        );
    }

    #[test]
    fn a_host_module_outranks_a_native_module() {
        let runtime = Runtime::<RustPython>::builder()
            .bind(ModuleSpec::new("geometry").constant("api_version", 1))
            .bind_native(NativeModule::new("geometry", Fixtures::native_module("geometry", 99)))
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        guest.exec("import geometry").unwrap();

        assert_eq!(
            guest
                .eval::<i64>("geometry.api_version")
                .unwrap(),
            1
        );
        assert!(
            !guest
                .eval::<bool>("hasattr(geometry, 'answer')")
                .unwrap(),
        );
    }

    #[test]
    fn a_native_module_outranks_a_bundle() {
        let runtime = Runtime::<RustPython>::builder()
            .bind_native(NativeModule::new("fastmath", Fixtures::native_module("fastmath", 42)))
            .bundle(Bundle::single("fastmath", "answer = 1\n").unwrap())
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        guest.exec("import fastmath").unwrap();

        assert_eq!(
            guest
                .eval::<i64>("fastmath.answer")
                .unwrap(),
            42
        );
    }

    #[test]
    fn a_host_initializer_runs_once_per_guest_build() {
        let calls = Rc::new(Cell::new(0));
        let runtime = Runtime::<RustPython>::builder()
            .bind(HostLibrary::new().initialize(HostInitializer::new({
                let calls = calls.clone();

                move |_| {
                    calls.set(calls.get() + 1);

                    Ok::<_, Error>(())
                }
            })))
            .build()
            .unwrap();

        assert_eq!(calls.get(), 0);

        let _first = runtime.guest().build().unwrap();

        assert_eq!(calls.get(), 1);

        let _second = runtime.guest().build().unwrap();

        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn a_native_initializer_runs_at_guest_build() {
        let calls = Rc::new(Cell::new(0));
        let runtime = Runtime::<RustPython>::builder()
            .bind_native(NativeLibrary::new().initialize(NativeInitializer::new({
                let calls = calls.clone();

                move |_| {
                    calls.set(calls.get() + 1);

                    Ok::<_, Error>(())
                }
            })))
            .build()
            .unwrap();

        let _guest = runtime.guest().build().unwrap();

        assert_eq!(calls.get(), 1);
    }
}
