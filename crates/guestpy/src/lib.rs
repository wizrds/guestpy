//! Lets a Rust application load and run Python code inside an isolated interpreter, call the
//! functions and read the values that code defines, and expose Rust functions, classes, and
//! modules for that Python code to call back into. This crate has no interpreter of its own; it
//! runs on top of a pluggable backend, so the same host code works unchanged against either of two
//! interchangeable backends: real CPython through
//! [`guestpy::pyo3::CPython`](crate::pyo3::CPython) (the `pyo3` Cargo feature), or RustPython, a
//! Python interpreter written entirely in Rust, through
//! [`guestpy::rustpython::RustPython`](crate::rustpython::RustPython) (the `rustpython` Cargo
//! feature).
//!
//! Every guestpy type is generic over a backend type parameter. The backend selects the Python
//! interpreter that runs guest code. This crate re-exports the core API and macros so applications
//! normally depend only on `guestpy`.
//!
//! Enabling the `pyo3` Cargo feature compiles in and re-exports
//! [`guestpy_pyo3`](guestpy_pyo3) as [`pyo3`](crate::pyo3); the `rustpython` feature does
//! the same for [`guestpy_rustpython`](guestpy_rustpython) as
//! [`rustpython`](crate::rustpython). An application depends only on this crate, with the
//! matching feature enabled, to use either backend.
//!
//! # Runtimes, guests, and isolation
//!
//! A [`guestpy_core::runtime::Runtime`](guestpy_core::runtime::Runtime) owns the selected backend.
//! Each guest built from it has an isolated interpreter:
//!
//! ```ignore
//! use guestpy::prelude::*;
//! use guestpy::pyo3::CPython;
//!
//! let runtime = Runtime::<CPython>::builder()
//!     .build()?;
//! let first = runtime.guest().build()?;
//! let second = runtime.guest().build()?;
//!
//! first.exec("name = 'first'")?;
//! second.exec("name = 'second'")?;
//!
//! assert_eq!(first.globals()?.item::<String, _>("name")?, "first");
//! assert_eq!(second.globals()?.item::<String, _>("name")?, "second");
//! ```
//!
//! Bind host libraries, native libraries, and source bundles to a runtime for all of its guests, or
//! to a guest builder for one guest. A builder can also deny imports by name.
//!
//! # Execution control
//!
//! Configure a timeout and cancellation signal when an application needs to stop guest execution:
//!
//! ```ignore
//! use guestpy::prelude::*;
//! use guestpy::pyo3::CPython;
//!
//! let cancellation = Cancellation::new();
//! let runtime = Runtime::<CPython>::builder()
//!     .timeout(std::time::Duration::from_millis(50))
//!     .cancellation(cancellation.clone())
//!     .build()?;
//! let guest = runtime.guest().build()?;
//!
//! assert!(matches!(guest.exec("while True: pass"), Err(Error::Timeout)));
//!
//! cancellation.cancel();
//!
//! assert!(matches!(guest.eval::<i64>("1 + 1"), Err(Error::Cancelled)));
//! ```
//!
//! Use the selected backend's configuration type with
//! [`guestpy_core::runtime::RuntimeBuilder::config`](guestpy_core::runtime::RuntimeBuilder::config)
//! for interpreter-specific settings.
//!
//! # Loading guest code
//!
//! [`guestpy_core::guest::Guest::exec`](guestpy_core::guest::Guest::exec) runs Python statements.
//! [`guestpy_core::guest::Guest::eval`](guestpy_core::guest::Guest::eval) evaluates an expression.
//! [`guestpy_core::guest::Guest::guest_module`](guestpy_core::guest::Guest::guest_module) loads a
//! named module and returns a [`guestpy_core::handle::module::Module`](guestpy_core::handle::module::Module)
//! for dynamic access to its exports:
//!
//! ```ignore
//! use guestpy::prelude::*;
//! use guestpy::pyo3::CPython;
//!
//! let guest = Runtime::<CPython>::builder()
//!     .build()?
//!     .guest()
//!     .build()?;
//!
//! let module = guest.guest_module(
//!     "dynamic",
//!     r#"
//! settings = {'prefix': 'hello'}
//!
//! def greet(name):
//!     return f"{settings['prefix']} {name}"
//! "#,
//! )?;
//!
//! assert_eq!(
//!     module.object("settings")?.get::<String>("prefix")?,
//!     "hello",
//! );
//! assert_eq!(
//!     module.function("greet")?.call::<_, String>(("Ada",))?,
//!     "hello Ada",
//! );
//! ```
//!
//! ## Loading several modules and packages together with Bundle
//!
//! Use [`guestpy_core::bundle::Bundle`](guestpy_core::bundle::Bundle) when guest code is a package
//! or spans several modules:
//!
//! ```ignore
//! use guestpy::prelude::*;
//! use guestpy::pyo3::CPython;
//!
//! let bundle = Bundle::builder()
//!     .module(
//!         "app.main",
//!         r#"
//! from app.util import double
//!
//! def run():
//!     return double(21)
//! "#,
//!     )
//!     .package("app", "")
//!     .module("app.util", "def double(value):\n    return value * 2\n")
//!     .build()?;
//!
//! let guest = Runtime::<CPython>::builder()
//!     .build()?
//!     .guest()
//!     .build()?;
//! let module = guest.load(&bundle)?;
//!
//! assert_eq!(module.function("run")?.call::<_, i64>(())?, 42);
//! ```
//!
//! Build a bundle from modules, packages, data, or a complete installed Python directory, then load
//! or bind it for guest imports. Pure Python files remain portable across backends. Compiled native
//! modules must match the selected backend, interpreter ABI, operating system, and architecture.
//! CPython native modules are process-global, so separate guests cannot load different binaries
//! under the same dotted module name. RustPython reports an unsupported error only when guest code
//! imports a bundled native module. The `embedded` and `tokio` features provide embedded and
//! filesystem bundle construction when an application needs them.
//!
//! # Typed guest facades
//!
//! [`guestpy::guest_module!`](crate::guest_module) generates a typed Rust facade for a known guest
//! module interface:
//!
//! ```ignore
//! use guestpy::prelude::*;
//! use guestpy::pyo3::CPython;
//!
//! guestpy::guest_module! {
//!     pub module Math {
//!         fn add(left: i64, right: i64) -> i64;
//!
//!         value answer: i64;
//!     }
//! }
//!
//! let guest = Runtime::<CPython>::builder()
//!     .build()?
//!     .guest()
//!     .build()?;
//! let math = Math::from(
//!     guest.guest_module(
//!         "math",
//!         r#"
//! def add(left, right):
//!     return left + right
//!
//! answer = 42
//! "#,
//!     )?,
//! );
//!
//! assert_eq!(math.add(20, 22)?, 42);
//! assert_eq!(math.answer()?, 42);
//! ```
//!
//! Declare the exports the host needs. The generated facade converts each result into the descriptor
//! type declared in the macro.
//!
//! ## Typed guest classes
//!
//! [`guestpy::guest_class!`](crate::guest_class) generates a typed Rust facade for a known
//! Python class or instance interface:
//!
//! ```ignore
//! use guestpy::prelude::*;
//! use guestpy::pyo3::CPython;
//!
//! guestpy::guest_class! {
//!     pub class Client {
//!         fn get(path: String) -> Response<B>;
//!
//!         value prefix: String;
//!     }
//! }
//!
//! guestpy::guest_class! {
//!     pub class Response {
//!         fn status() -> i64;
//!     }
//! }
//!
//! guestpy::guest_module! {
//!     pub module Plugin {
//!         #[guestpy(name = "Client")]
//!         value client_class: Class<B, Client<B>>;
//!
//!         value default_client: Client<B>;
//!     }
//! }
//!
//! let guest = Runtime::<CPython>::builder()
//!     .build()?
//!     .guest()
//!     .build()?;
//! let plugin = Plugin::from(
//!     guest.guest_module(
//!         "plugin",
//!         r#"
//! class Response:
//!     def __init__(self, code):
//!         self.code = code
//!
//!     def status(self):
//!         return self.code
//!
//! class Client:
//!     def __init__(self, prefix):
//!         self.prefix = prefix
//!
//!     def get(self, path):
//!         return Response(len(self.prefix + path))
//!
//! default_client = Client('default:')
//! "#,
//!     )?,
//! );
//!
//! let client = plugin
//!     .client_class()?
//!     .construct(("api:".to_owned(),))?;
//!
//! assert_eq!(client.prefix()?, "api:");
//! assert_eq!(client.get("users".to_owned())?.status()?, 9);
//! ```
//!
//! A [`guestpy_core::handle::Class`](guestpy_core::handle::Class) remembers the
//! descriptor used to convert its constructed instance. A generated class facade wraps a
//! [`guestpy_core::handle::Instance`](guestpy_core::handle::Instance) and calls
//! Python attributes normally, so the same facade works for Python-defined classes,
//! host-injected classes, and Python subclasses. Use
//! [`guestpy_core::handle::Object`](guestpy_core::handle::Object) for
//! unrestricted dynamic access, the default
//! [`guestpy_core::handle::Instance`](guestpy_core::handle::Instance) for a
//! dynamic instance view, and a payload-typed
//! [`guestpy_core::handle::Instance`](guestpy_core::handle::Instance) when the
//! live object carries a checked host payload that Rust must borrow.
//!
//! # Guest-side async
//!
//! Convert a returned Python coroutine to
//! [`guestpy_core::handle::coroutine::Coroutine`](guestpy_core::handle::coroutine::Coroutine) or
//! [`guestpy_core::handle::coroutine::Awaitable`](guestpy_core::handle::coroutine::Awaitable), then
//! await it from async Rust code:
//!
//! ```ignore
//! use guestpy::prelude::*;
//! use guestpy::pyo3::CPython;
//!
//! let guest = Runtime::<CPython>::builder()
//!     .build()?
//!     .guest()
//!     .build()?;
//!
//! guest.exec(
//!     r#"
//! import asyncio
//!
//! async def double(value):
//!     await asyncio.sleep(0)
//!     return value * 2
//! "#,
//! )?;
//!
//! let doubled = guest
//!     .globals()?
//!     .item::<Function<_>, _>("double")?
//!     .call::<_, Coroutine<_, i64>>((21,))?
//!     .await?;
//!
//! assert_eq!(doubled, 42);
//! ```
//!
//! Use [`guestpy_core::handle::coroutine::Coroutine`](guestpy_core::handle::coroutine::Coroutine)
//! when the result must be awaitable. Use
//! [`guestpy_core::handle::coroutine::Awaitable`](guestpy_core::handle::coroutine::Awaitable) when
//! it may be a direct value or an awaitable. Other host and guest operations are synchronous.
//!
//! # Plain Rust data
//!
//! Derive [`guestpy::ToGuest`](crate::ToGuest) and [`guestpy::FromGuest`](crate::FromGuest) for
//! serde-compatible data under the `serde` Cargo feature:
//!
//! ```ignore
//! use guestpy::prelude::*;
//!
//! #[derive(
//!     Debug,
//!     PartialEq,
//!     serde::Serialize,
//!     serde::Deserialize,
//!     guestpy::ToGuest,
//!     guestpy::FromGuest,
//! )]
//! struct Request {
//!     #[serde(rename = "userId")]
//!     user_id: u64,
//!     note: Option<String>,
//! }
//! ```
//!
//! The derives follow the type's serde representation. Standard Rust primitives, options,
//! collections, arrays, tuples, and iterable values also cross the boundary directly. Use a host
//! class when guest code needs to retain Rust object identity.
//!
//! # Host classes and modules
//!
//! [`guestpy::host_class`](crate::host_class) exposes an ordinary Rust type as a Python class. Mark
//! the constructor, methods, and properties guest code needs:
//!
//! ```ignore
//! use guestpy::prelude::*;
//!
//! struct Vector2 {
//!     x: f64,
//!     y: f64,
//! }
//!
//! #[guestpy::host_class]
//! impl Vector2 {
//!     #[guestpy(constructor)]
//!     fn new(x: f64, y: f64) -> Result<Self, Error> {
//!         Ok(Self { x, y })
//!     }
//!
//!     #[guestpy(method)]
//!     fn length(&self) -> Result<f64, Error> {
//!         Ok(self.x.hypot(self.y))
//!     }
//!
//!     #[guestpy(get)]
//!     fn x(&self) -> Result<f64, Error> {
//!         Ok(self.x)
//!     }
//!
//! }
//! ```
//!
//! A host class whose payload holds guest values rather than plain Rust data is written by hand
//! instead of through the macro.
//! [`HostClass`](guestpy_core::host::class::HostClass) carries the class identity, and
//! [`HostClassDefinition`](guestpy_core::host::class::HostClassDefinition) carries construction and
//! member registration against one backend, so the type itself can be generic over `B` and store
//! handles such as [`Object<B>`](guestpy_core::handle::Object):
//!
//! ```ignore
//! use guestpy::prelude::*;
//!
//! struct Envelope<B: Backend> {
//!     payload: Object<B>,
//! }
//!
//! impl<B: Backend> HostClass for Envelope<B> {
//!     const NAME: &'static str = "Envelope";
//! }
//!
//! impl<B> HostClassDefinition<B> for Envelope<B>
//! where
//!     B: Backend + BackendValues + BackendCallables + BackendClasses,
//! {
//!     fn construct<'py>(enter: &Enter<'py, B>, args: Args<'py, B>) -> Result<Self, Error> {
//!         let envelope = Self {
//!             payload: args.required::<Object<B>>(enter, 0, "payload")?,
//!         };
//!
//!         args.finish()?;
//!
//!         Ok(envelope)
//!     }
//!
//!     fn build(builder: &mut ClassBuilder<B, Self>) {
//!         builder.getter("payload", |envelope, _| Ok(envelope.payload.clone()));
//!     }
//! }
//! ```
//!
//! Register it by naming the backend the module is built for, as in
//! `ModuleSpec::<CPython>::new("host_mail").class::<Envelope<CPython>>()`.
//!
//! The macro also supports mutable methods, class-level members, Python protocol methods,
//! inheritance, and asynchronous host work. Use [`guestpy::host_module`](crate::host_module) to
//! expose functions and classes through a Python-importable module. Combine modules with
//! [`guestpy_core::host::library::HostLibrary`](guestpy_core::host::library::HostLibrary) and bind
//! the library to a runtime or guest builder.
//!
//! Returning a host-class value to Rust preserves the live guest instance rather than cloning
//! the Rust payload. Use
//! [`guestpy_core::handle::Instance::borrow_with`](guestpy_core::handle::Instance::borrow_with)
//! or
//! [`guestpy_core::handle::Instance::borrow_with_mut`](guestpy_core::handle::Instance::borrow_with_mut)
//! when the host needs direct payload access; ordinary facade calls still use Python dispatch.
//!
//! Use [`guestpy_core::native::NativeModule`](guestpy_core::native::NativeModule) and
//! [`guestpy_core::native::NativeLibrary`](guestpy_core::native::NativeLibrary) only for a
//! backend-specific integration that cannot be expressed as a host module.
//!
//! # Errors
//!
//! Every operation returns `Result<T, [`guestpy_core::errors::Error`](guestpy_core::errors::Error)>`.
//! Match a known Python exception when the host can recover from it, and propagate every other
//! error:
//!
//! ```ignore
//! use guestpy::prelude::*;
//! use guestpy::pyo3::CPython;
//!
//! fn read_count(guest: &Guest<CPython>) -> Result<Option<i64>, Error> {
//!     match guest.eval::<i64>("int('not a number')") {
//!         Ok(count) => Ok(Some(count)),
//!         Err(Error::Guest(exception)) if exception.matches("ValueError") => {
//!             eprintln!(
//!                 "guest rejected the count: {}",
//!                 exception.message(),
//!             );
//!
//!             Ok(None)
//!         }
//!         Err(error) => Err(error),
//!     }
//! }
//! ```
//!
//! [`guestpy_core::errors::GuestException::matches`](guestpy_core::errors::GuestException::matches)
//! recognizes the exception's Python inheritance hierarchy. Use
//! [`guestpy_core::errors::GuestException::qualified_name`](guestpy_core::errors::GuestException::qualified_name),
//! [`guestpy_core::errors::GuestException::message`](guestpy_core::errors::GuestException::message),
//! and [`guestpy_core::errors::GuestException::traceback`](guestpy_core::errors::GuestException::traceback)
//! when reporting a guest failure.

#[allow(unused_extern_crates)]
extern crate self as guestpy;

pub mod prelude;

pub use guestpy_core::*;
pub use guestpy_macros::{FromGuest, ToGuest, guest_class, guest_module, host_class, host_module};

#[cfg(feature = "embedded")]
pub use guestpy_macros::bundle;

#[cfg(feature = "rustpython")]
pub mod rustpython {
    pub use guestpy_rustpython::*;
}

#[cfg(feature = "pyo3")]
pub mod pyo3 {
    pub use guestpy_pyo3::*;
}

#[cfg(test)]
mod tests {
    use guestpy::pyo3::CPython;
    use guestpy::rustpython::RustPython;

    use crate::{
        marshal::{FromGuest, ToGuest},
        prelude::*,
    };

    macro_rules! parameterized {
        (
            $(
                $(#[$attribute:meta])*
                async fn $name:ident() $body:block
            )*
        ) => {
            #[cfg(feature = "rustpython")]
            mod rustpython {
                use super::*;

                type B = RustPython;

                $(
                    $(#[$attribute])*
                    #[test]
                    fn $name() {
                        block_on(async $body)
                    }
                )*
            }

            #[cfg(feature = "pyo3")]
            mod pyo3 {
                use super::*;

                type B = CPython;

                $(
                    $(#[$attribute])*
                    #[test]
                    fn $name() {
                        block_on(async $body)
                    }
                )*
            }
        };
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        tokio::task::LocalSet::new().block_on(
            &tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap(),
            future,
        )
    }

    guestpy::guest_module! {
        pub module Math {
            fn add(left: i64, right: i64) -> i64;

            value answer: i64;
        }
    }

    guestpy::guest_class! {
        pub class Client {
            fn get(path: String) -> Response<B>;

            value prefix: String;
        }
    }

    guestpy::guest_class! {
        pub class Response {
            fn status() -> i64;
        }
    }

    guestpy::guest_module! {
        pub module Plugin {
            #[guestpy(name = "Client")]
            value client_class: Class<B, Client<B>>;

            value default_client: Client<B>;

            fn forward(client: Client<B>, path: String) -> Response<B>;
            fn identity(client: Client<B>) -> Client<B>;
        }
    }

    struct ManualClient<B: Backend> {
        instance: Instance<B>,
    }

    impl<B: Backend> ManualClient<B> {
        fn new(instance: Instance<B>) -> Self {
            Self { instance }
        }

        fn instance(&self) -> &Instance<B> {
            &self.instance
        }

        fn into_instance(self) -> Instance<B> {
            self.instance
        }
    }

    impl<B> ManualClient<B>
    where
        B: Backend + BackendValues,
    {
        fn get(&self, path: String) -> Result<Response<B>, Error> {
            self.instance
                .call_method::<_, Response<B>>("get", (path,))
        }
    }

    impl<B: Backend> From<Instance<B>> for ManualClient<B> {
        fn from(instance: Instance<B>) -> Self {
            Self::new(instance)
        }
    }

    impl<B: Backend> From<ManualClient<B>> for Instance<B> {
        fn from(val: ManualClient<B>) -> Self {
            val.into_instance()
        }
    }

    impl<B: Backend> FromGuest<B> for ManualClient<B> {
        type Owned = Self;

        fn from_guest<'py>(
            enter: &Enter<'py, B>,
            value: <B as Backend>::Value<'py>,
        ) -> Result<Self::Owned, Error> {
            <Instance<B> as FromGuest<B>>::from_guest(enter, value).map(Self::new)
        }
    }

    impl<B: Backend> ToGuest<B> for ManualClient<B> {
        fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<<B as Backend>::Value<'py>, Error> {
            ToGuest::to_guest(self.into_instance(), enter)
        }
    }

    #[derive(
        Debug, PartialEq, serde::Serialize, serde::Deserialize, crate::ToGuest, crate::FromGuest,
    )]
    struct Request {
        #[serde(rename = "userId")]
        user_id: u64,
        note: Option<String>,
    }

    struct Vector2 {
        x: f64,
        y: f64,
    }

    #[crate::host_class(rename_all = "camelCase")]
    impl Vector2 {
        #[guestpy(constructor)]
        fn new(x: f64, y: f64) -> Result<Self, Error> {
            Ok(Self { x, y })
        }

        #[guestpy(method)]
        fn length(&self) -> Result<f64, Error> {
            Ok(self.x.hypot(self.y))
        }

        #[guestpy(method, name = "moveBy")]
        fn translate(&mut self, dx: f64, dy: f64) -> Result<(), Error> {
            self.x += dx;
            self.y += dy;

            Ok(())
        }

        #[guestpy(get)]
        fn x(&self) -> Result<f64, Error> {
            Ok(self.x)
        }

        #[guestpy(dunder = "__repr__")]
        fn repr(&self) -> Result<String, Error> {
            Ok(format!("Vector2({}, {})", self.x, self.y))
        }

        #[guestpy(static_method)]
        fn origin() -> Result<Self, Error> {
            Ok(Self { x: 0.0, y: 0.0 })
        }
    }

    struct Geometry;

    #[crate::host_module(name = "host_geometry", classes(Vector2))]
    impl Geometry {
        #[guestpy(function)]
        fn hypot(left: f64, right: f64) -> Result<f64, Error> {
            Ok(left.hypot(right))
        }
    }

    guestpy::guest_class! {
        #[guestpy(payload = Vector2)]
        pub class HostVector {
            fn length() -> f64;

            value x: f64;
        }
    }

    parameterized! {
        async fn quick_start_example_adds_two_numbers() {
            let guest = Runtime::<B>::builder()
                .build()
                .unwrap()
                .guest()
                .build()
                .unwrap();

            guest
                .exec("def add(left, right):\n    return left + right\n")
                .unwrap();

            assert_eq!(
                guest
                    .globals()
                    .unwrap()
                    .item::<Function<_>, _>("add")
                    .unwrap()
                    .call::<_, i64>((20, 22))
                    .unwrap(),
                42,
            );
        }

        async fn separate_guests_do_not_share_state() {
            let runtime = Runtime::<B>::builder().build().unwrap();
            let first = runtime.guest().build().unwrap();
            let second = runtime.guest().build().unwrap();

            first.exec("name = 'first'").unwrap();
            second.exec("name = 'second'").unwrap();

            assert_eq!(
                first
                    .globals()
                    .unwrap()
                    .item::<String, _>("name")
                    .unwrap(),
                "first",
            );
            assert_eq!(
                second
                    .globals()
                    .unwrap()
                    .item::<String, _>("name")
                    .unwrap(),
                "second",
            );
        }

        async fn dynamic_handle_api_reads_and_calls_a_loaded_module() {
            let guest = Runtime::<B>::builder()
                .build()
                .unwrap()
                .guest()
                .build()
                .unwrap();

            let module = guest
                .guest_module(
                    "dynamic",
                    "settings = {'prefix': 'hello'}\n\n\
                    def greet(name):\n    return f\"{settings['prefix']} {name}\"\n",
                )
                .unwrap();

            assert_eq!(
                module.object("settings").unwrap().item::<String, _>("prefix").unwrap(),
                "hello",
            );
            assert_eq!(
                module.function("greet").unwrap().call::<_, String>(("Ada",)).unwrap(),
                "hello Ada",
            );
        }

        async fn bundle_loads_a_package_of_several_modules() {
            let bundle = Bundle::builder()
                .module(
                    "app.main",
                    "from app.util import double\n\ndef run():\n    return double(21)\n",
                )
                .package("app", "from .main import run\n")
                .module("app.util", "def double(value):\n    return value * 2\n")
                .build()
                .unwrap();

            let guest = Runtime::<B>::builder()
                .build()
                .unwrap()
                .guest()
                .build()
                .unwrap();

            let module = guest.load(&bundle).unwrap();

            assert_eq!(module.function("run").unwrap().call::<_, i64>(()).unwrap(), 42);
        }

        async fn typed_facade_calls_and_reads_guest_exports() {
            let guest = Runtime::<B>::builder()
                .build()
                .unwrap()
                .guest()
                .build()
                .unwrap();

            let math = Math::from(
                guest
                    .guest_module(
                        "math",
                        "def add(left, right):\n    return left + right\n\nanswer = 42\n",
                    )
                    .unwrap(),
            );

            assert_eq!(math.add(20, 22).unwrap(), 42);
            assert_eq!(math.answer().unwrap(), 42);
        }

        async fn typed_class_facades_preserve_dispatch_conversions_and_identity() {
            let guest = Runtime::<B>::builder()
                .build()
                .unwrap()
                .guest()
                .build()
                .unwrap();
            let module = guest
                .guest_module(
                    "plugin",
                    r#"
class Response:
    def __init__(self, code):
        self.code = code

    def status(self):
        return self.code

class Client:
    def __init__(self, prefix):
        self.prefix = prefix

    def get(self, path):
        return Response(len(self.prefix + path))

default_client = Client('default:')

def forward(client, path):
    return client.get(path)

def identity(client):
    return client
"#,
                )
                .unwrap();
            let plugin = Plugin::from(module.clone());
            let client = plugin
                .client_class()
                .unwrap()
                .construct(("typed:".to_owned(),))
                .unwrap();

            assert_eq!(client.prefix().unwrap(), "typed:");
            assert_eq!(
                client
                    .get("path".to_owned())
                    .unwrap()
                    .status()
                    .unwrap(),
                10,
            );
            assert_eq!(
                plugin
                    .forward(client.clone(), "value".to_owned())
                    .unwrap()
                    .status()
                    .unwrap(),
                11,
            );

            let exported = plugin.default_client().unwrap();
            let retained = exported.instance().clone();
            let returned = plugin.identity(exported.clone()).unwrap();

            assert!(
                retained
                    .value()
                    .ptr_eq(&returned.instance().value()),
            );
            assert!(
                <Client<B> as AsRef<Instance<B>>>::as_ref(&returned)
                    .value()
                    .ptr_eq(&returned.instance().value()),
            );

            let instance = <Client<B> as Into<Instance<B>>>::into(returned.clone());
            let rebuilt = Client::<B>::from(instance);

            assert_eq!(rebuilt.prefix().unwrap(), "default:");
            assert_eq!(
                rebuilt
                    .clone()
                    .into_instance()
                    .get::<String>("prefix")
                    .unwrap(),
                "default:",
            );

            let manual = module.get::<ManualClient<B>>("default_client").unwrap();

            assert_eq!(
                manual
                    .get("manual".to_owned())
                    .unwrap()
                    .status()
                    .unwrap(),
                14,
            );
            assert_eq!(
                manual
                    .instance()
                    .get::<String>("prefix")
                    .unwrap(),
                "default:",
            );
        }

        async fn guest_coroutine_is_awaited_as_a_rust_future() {
            let guest = Runtime::<B>::builder()
                .build()
                .unwrap()
                .guest()
                .build()
                .unwrap();

            guest
                .exec(
                    "import asyncio\n\n\
                    async def double(value):\n    await asyncio.sleep(0)\n    return value * 2\n",
                )
                .unwrap();

            let doubled = guest
                .globals()
                .unwrap()
                .item::<Function<_>, _>("double")
                .unwrap()
                .call::<_, Coroutine<_, i64>>((21,))
                .unwrap()
                .await
                .unwrap();

            assert_eq!(doubled, 42);
        }

        async fn derived_struct_roundtrips_through_a_guest_function() {
            let guest = Runtime::<B>::builder()
                .build()
                .unwrap()
                .guest()
                .build()
                .unwrap();

            guest
                .exec(
                    "def normalize(request):\n    \
                    return {'userId': request['userId'], 'note': request['note']}\n",
                )
                .unwrap();

            assert_eq!(
                guest
                    .globals()
                    .unwrap()
                    .item::<Function<_>, _>("normalize")
                    .unwrap()
                    .call::<_, Request>((Request { user_id: 42, note: None },))
                    .unwrap(),
                Request { user_id: 42, note: None },
            );
        }

        async fn host_class_and_host_module_are_visible_to_guest_code() {
            let guest = Runtime::<B>::builder()
                .bind(Geometry::module())
                .build()
                .unwrap()
                .guest()
                .build()
                .unwrap();

            let module = guest
                .guest_module(
                    "geometry_demo",
                    "from host_geometry import Vector2, hypot\n\n\
                    def describe():\n    \
                    vector = Vector2(3, 4)\n    \
                    vector.moveBy(1, 2)\n    \
                    return (vector.x, repr(vector), hypot(5, 12))\n",
                )
                .unwrap();

            assert_eq!(
                module.function("describe").unwrap().call::<_, (f64, String, f64)>(()).unwrap(),
                (4.0, "Vector2(4, 6)".to_string(), 13.0),
            );
        }

        async fn payload_facade_uses_python_dispatch_and_inferred_borrowing() {
            let guest = Runtime::<B>::builder()
                .bind(Geometry::module())
                .build()
                .unwrap()
                .guest()
                .build()
                .unwrap();

            guest.exec("import host_geometry").unwrap();

            let vector = guest
                .eval::<HostVector<B>>("host_geometry.Vector2(3, 4)")
                .unwrap();

            assert_eq!(vector.length().unwrap(), 5.0);
            assert_eq!(vector.x().unwrap(), 3.0);
            assert_eq!(
                vector
                    .instance()
                    .borrow_with(|payload| payload.x.hypot(payload.y))
                    .unwrap(),
                5.0,
            );
        }
    }
}
