//! RustPython module operations.

use guestpy_core::{
    backend::{BackendModules, Tok, Val},
    errors::Error,
};
use rustpython_vm::{builtins::PyCode, compiler::Mode, scope::Scope};

use crate::{
    engine::{Engine, RustPython},
    errors::NativeErrors,
    values::AsDict,
};

impl BackendModules for RustPython {
    fn new_module<'py>(
        vm: Tok<'py, Self>,
        name: &str,
        dict: Val<'py, Self>,
        doc: Option<&str>,
    ) -> Result<Val<'py, Self>, Error> {
        Ok(vm
            .new_module(name, dict.as_dict()?.to_owned(), doc.map(|doc| vm.ctx.new_str(doc)))
            .into())
    }

    fn compile<'py>(
        vm: Tok<'py, Self>,
        source: &str,
        filename: &str,
    ) -> Result<Val<'py, Self>, Error> {
        vm.compile(source, Mode::Exec, filename.to_owned())
            .map(Into::into)
            .map_err(|error| RustPython::guest(vm, error.into_pyexception(vm, Some(source))))
    }

    fn exec_code<'py>(
        vm: Tok<'py, Self>,
        code: &Val<'py, Self>,
        globals: &Val<'py, Self>,
    ) -> Result<(), Error> {
        vm.run_code_obj(
            code.clone()
                .downcast::<PyCode>()
                .map_err(|_| Error::unexpected("compiled object was not a code object"))?,
            Scope::new(None, globals.as_dict()?.to_owned()),
        )
        .map(|_| ())
        .map_err(|error| RustPython::guest(vm, error))
    }

    fn eval<'py>(
        vm: Tok<'py, Self>,
        source: &str,
        filename: &str,
        globals: &Val<'py, Self>,
    ) -> Result<Val<'py, Self>, Error> {
        let code = vm
            .compile(source, Mode::Eval, filename.to_owned())
            .map_err(|error| RustPython::guest(vm, error.into_pyexception(vm, Some(source))))?;

        vm.run_code_obj(code, Scope::new(None, globals.as_dict()?.to_owned()))
            .map_err(|error| RustPython::guest(vm, error))
    }

    fn builtins_dict<'py>(vm: Tok<'py, Self>) -> Result<Val<'py, Self>, Error> {
        Ok(vm.builtins.dict().into())
    }

    fn install_dispatcher<'py>(
        vm: Tok<'py, Self>,
        engine: &Engine,
        dispatcher: Val<'py, Self>,
    ) -> Result<(), Error> {
        if engine.claim_dispatcher() {
            return Ok(());
        }

        vm.builtins
            .set_attr("__import__", dispatcher, vm)
            .map_err(|error| RustPython::guest(vm, error))
    }

    fn real_import<'py>(vm: Tok<'py, Self>) -> Result<Val<'py, Self>, Error> {
        vm.builtins
            .get_attr("__import__", vm)
            .map_err(|error| RustPython::guest(vm, error))
    }
}

#[cfg(test)]
mod tests {
    use guestpy_core::{
        bundle::Bundle,
        errors::{Error, GuestException},
        handle::ObjectProtocol,
        host::module::ModuleSpec,
        runtime::Runtime,
    };

    use crate::engine::RustPython;

    struct Fixtures;

    impl Fixtures {
        fn plugin() -> Bundle {
            Bundle::builder()
                .package(
                    "plugin",
                    "from .handlers import http\nfrom . import util\n\nNAME = \"plugin\"\n\ndef entry(value):\n    return util.decorate(http.tag(value))\n",
                )
                .module(
                    "plugin.util",
                    "PREFIX = \"u:\"\n\ndef decorate(value):\n    return PREFIX + value\n",
                )
                .package("plugin.handlers", "")
                .module(
                    "plugin.handlers.http",
                    "from ..util import PREFIX\n\ndef tag(value):\n    return \"h:\" + value + PREFIX\n",
                )
                .build()
                .unwrap()
        }

        fn geometry(version: i64) -> ModuleSpec<RustPython> {
            ModuleSpec::new("geometry")
                .doc("Geometry helpers.")
                .constant("api_version", version)
                .function("hypot", |enter, args| {
                    Ok::<_, Error>(
                        args.required::<f64>(enter, 0, "x")?
                            .hypot(args.required::<f64>(enter, 1, "y")?),
                    )
                })
                .getter("tolerance", |_| Ok::<_, Error>(1e-9_f64))
        }

        fn impostor() -> Bundle {
            Bundle::single("geometry", "api_version = 99\n").unwrap()
        }
    }

    struct Raises;

    impl Raises {
        fn guest(error: Error) -> GuestException {
            match error {
                Error::Guest(exception) => *exception,
                other => panic!("expected a guest exception, got: {other}"),
            }
        }
    }

    #[test]
    fn binds_at_the_runtime_level() {
        let runtime = Runtime::<RustPython>::builder()
            .bind(Fixtures::geometry(1))
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
        assert_eq!(
            guest
                .eval::<f64>("geometry.hypot(3.0, 4.0)")
                .unwrap(),
            5.0
        );
        assert_eq!(
            guest
                .host_module("geometry")
                .unwrap()
                .get::<i64>("api_version")
                .unwrap(),
            1,
        );
    }

    #[test]
    fn host_module_is_the_guests_own_module() {
        let runtime = Runtime::<RustPython>::builder()
            .bind(Fixtures::geometry(1))
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        guest.exec("import geometry").unwrap();
        guest
            .host_module("geometry")
            .unwrap()
            .set::<i64>("marker", 7)
            .unwrap();

        assert!(
            guest
                .eval::<bool>("geometry.marker == 7")
                .unwrap()
        );
    }

    #[test]
    fn binds_at_the_guest_level() {
        let runtime = Runtime::<RustPython>::builder()
            .build()
            .unwrap();
        let guest = runtime
            .guest()
            .bind(Fixtures::geometry(1))
            .build()
            .unwrap();
        let other = runtime.guest().build().unwrap();

        guest.exec("import geometry").unwrap();

        assert!(
            Raises::guest(
                other
                    .exec("import geometry")
                    .unwrap_err()
            )
            .matches("ImportError"),
        );
    }

    #[test]
    fn two_guests_get_two_module_objects() {
        let runtime = Runtime::<RustPython>::builder()
            .bind(Fixtures::geometry(1))
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();
        let other = runtime.guest().build().unwrap();

        guest
            .exec("import geometry\ngeometry.marker = 'a'\n")
            .unwrap();
        other
            .exec("import geometry\ngeometry.marker = 'b'\n")
            .unwrap();

        assert_eq!(
            guest
                .eval::<String>("geometry.marker")
                .unwrap(),
            "a"
        );
        assert_eq!(
            other
                .eval::<String>("geometry.marker")
                .unwrap(),
            "b"
        );
    }

    #[test]
    fn guest_level_binding_outranks_runtime_level() {
        let runtime = Runtime::<RustPython>::builder()
            .bind(Fixtures::geometry(1))
            .build()
            .unwrap();
        let guest = runtime
            .guest()
            .bind(Fixtures::geometry(2))
            .build()
            .unwrap();

        guest.exec("import geometry").unwrap();

        assert_eq!(
            guest
                .eval::<i64>("geometry.api_version")
                .unwrap(),
            2
        );
    }

    #[test]
    fn a_host_module_outranks_a_bundle() {
        let runtime = Runtime::<RustPython>::builder()
            .bind(Fixtures::geometry(1))
            .bundle(Fixtures::impostor())
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
    }

    #[test]
    fn a_module_getattr_reaches_a_host_getter() {
        let runtime = Runtime::<RustPython>::builder()
            .bind(Fixtures::geometry(1))
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        guest.exec("import geometry").unwrap();

        assert!(
            guest
                .eval::<f64>("geometry.tolerance")
                .unwrap()
                < 1.0e-8
        );
        assert!(
            guest
                .eval::<bool>("hasattr(geometry, 'tolerance')")
                .unwrap()
        );
        assert!(
            !guest
                .eval::<bool>("hasattr(geometry, 'nope')")
                .unwrap()
        );
    }

    #[test]
    fn denies_a_module() {
        let runtime = Runtime::<RustPython>::builder()
            .build()
            .unwrap();
        let denied = runtime
            .guest()
            .deny("sys")
            .build()
            .unwrap();
        let allowed = runtime.guest().build().unwrap();

        let error = Raises::guest(denied.exec("import sys").unwrap_err());

        assert!(error.matches("ImportError"));
        assert_eq!(error.name(), Some("sys"));
        allowed.exec("import sys").unwrap();

        let strict = Runtime::<RustPython>::builder()
            .deny("sys")
            .build()
            .unwrap();

        assert!(
            Raises::guest(
                strict
                    .guest()
                    .build()
                    .unwrap()
                    .exec("import sys")
                    .unwrap_err(),
            )
            .matches("ImportError"),
        );
    }

    #[test]
    fn imports_the_stdlib() {
        let runtime = Runtime::<RustPython>::builder()
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        guest.exec("import json").unwrap();

        assert_eq!(
            guest
                .eval::<String>("json.dumps({'a': 1})")
                .unwrap(),
            "{\"a\": 1}",
        );
    }

    #[test]
    fn loads_a_bundle() {
        let runtime = Runtime::<RustPython>::builder()
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();
        let plugin = guest.load(&Fixtures::plugin()).unwrap();

        assert_eq!(plugin.get::<String>("NAME").unwrap(), "plugin");
        assert_eq!(
            plugin
                .call_method::<_, String>("entry", ("7",))
                .unwrap(),
            "u:h:7u:",
        );

        guest
            .exec("import plugin.util")
            .unwrap();

        assert!(
            guest
                .eval::<bool>("plugin.util.PREFIX == 'u:'")
                .unwrap()
        );
    }

    #[test]
    fn import_from_finds_the_submodule() {
        let runtime = Runtime::<RustPython>::builder()
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        guest.load(&Fixtures::plugin()).unwrap();
        guest
            .exec("from plugin import handlers")
            .unwrap();

        assert!(
            guest
                .eval::<bool>("handlers.http.tag('x') == 'h:xu:'")
                .unwrap(),
        );
    }

    #[test]
    fn relative_imports_resolve() {
        let runtime = Runtime::<RustPython>::builder()
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        guest.load(&Fixtures::plugin()).unwrap();
        guest
            .exec("import plugin.util")
            .unwrap();
        assert!(
            guest
                .eval::<bool>("plugin.util.PREFIX == 'u:'")
                .unwrap()
        );

        let deep = Bundle::builder()
            .package("deep", "")
            .module("deep.mod", "from ... import nothing\n")
            .build()
            .unwrap();

        guest.load(&deep).unwrap();

        assert!(
            Raises::guest(
                guest
                    .exec("import deep.mod")
                    .unwrap_err()
            )
            .message()
            .contains("beyond top-level package"),
        );
    }

    #[test]
    fn sys_modules_is_untouched() {
        let runtime = Runtime::<RustPython>::builder()
            .bind(Fixtures::geometry(1))
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        guest.load(&Fixtures::plugin()).unwrap();
        guest.exec("import geometry").unwrap();
        guest.exec("import sys").unwrap();

        assert!(
            guest
                .eval::<bool>("'plugin' not in sys.modules")
                .unwrap()
        );
        assert!(
            guest
                .eval::<bool>("'plugin.util' not in sys.modules")
                .unwrap(),
        );
        assert!(
            guest
                .eval::<bool>("'geometry' not in sys.modules")
                .unwrap()
        );
    }

    #[test]
    fn guest_module_and_exec_and_eval() {
        let runtime = Runtime::<RustPython>::builder()
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();
        let solo = guest
            .guest_module("solo", "VALUE = 7\n")
            .unwrap();

        assert_eq!(solo.get::<i64>("VALUE").unwrap(), 7);

        guest.exec("import solo").unwrap();
        solo.set::<i64>("EXTRA", 9).unwrap();

        assert!(
            guest
                .eval::<bool>("solo.EXTRA == 9")
                .unwrap()
        );
        assert!(matches!(
            guest.guest_module("solo", "VALUE = 7\n"),
            Err(Error::NameInUse { .. }),
        ));
    }

    #[test]
    fn a_multi_root_bundle_cannot_be_loaded() {
        let runtime = Runtime::<RustPython>::builder()
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        assert!(matches!(
            guest.load(
                &Bundle::builder()
                    .module("alpha", "")
                    .module("beta", "")
                    .build()
                    .unwrap(),
            ),
            Err(Error::AmbiguousBundle { roots: 2 }),
        ));
    }

    #[test]
    fn failed_import_does_not_cache_a_partial_module() {
        let guest = Runtime::<RustPython>::builder()
            .bundle(
                Bundle::single(
                    "broken",
                    r#"
raise RuntimeError("broken import")
"#,
                )
                .unwrap(),
            )
            .build()
            .unwrap()
            .guest()
            .build()
            .unwrap();

        for _ in 0..2 {
            let error = Raises::guest(guest.exec("import broken").unwrap_err());

            assert!(error.matches("RuntimeError"));
            assert!(
                error
                    .message()
                    .contains("broken import")
            );
        }
    }
}
