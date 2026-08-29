pub mod callables;
pub mod classes;
pub mod coroutines;
pub mod exceptions;
pub mod interrupt;
pub mod library;
pub mod modules;
pub mod native_extensions;
pub mod values;

pub use callables::BackendCallables;
pub use classes::BackendClasses;
pub use coroutines::BackendCoroutines;
pub use exceptions::BackendExceptions;
pub use interrupt::BackendInterrupt;
pub use library::BackendLibrary;
pub use modules::BackendModules;
pub use native_extensions::{
    NativeExtensionContext,
    NativeExtensionLoader,
    NoNativeExtensions,
    PreparedNativeExtensions,
};
pub(crate) use native_extensions::PreparedNativeExtensionsOf;
pub use values::BackendValues;

use crate::errors::Error;
use std::fmt::Debug;

pub type Val<'py, B> = <B as Backend>::Value<'py>;
pub type Tok<'py, B> = <B as Backend>::Token<'py>;

pub trait Backend: Sized + 'static {
    type Engine;
    type Context;
    type Token<'py>: Copy;
    type Value<'py>: Clone + Debug;
    type Owned: Clone + 'static;
    type Config: Default;
    type NativeExtensions: NativeExtensionLoader<Self>;

    const NAME: &'static str;

    fn engine(config: Self::Config) -> Result<Self::Engine, Error>;
    fn shutdown(engine: Self::Engine) -> Result<(), Error>;

    fn enter<F, R>(engine: &Self::Engine, f: F) -> R
    where
        F: for<'py> FnOnce(Self::Token<'py>) -> R;

    fn new_context<'py>(
        token: Self::Token<'py>,
        globals: Self::Value<'py>,
        builtins: Self::Value<'py>,
    ) -> Self::Context;

    fn context_globals<'py>(token: Self::Token<'py>, context: &Self::Context) -> Self::Value<'py>;

    fn context_builtins<'py>(token: Self::Token<'py>, context: &Self::Context) -> Self::Value<'py>;

    fn detach<'py>(token: Self::Token<'py>, value: Self::Value<'py>) -> Self::Owned;
    fn attach<'py>(token: Self::Token<'py>, owned: &Self::Owned) -> Self::Value<'py>;
    fn release(owned: Self::Owned);

    fn owned_ptr_eq(first: &Self::Owned, second: &Self::Owned) -> bool;
}

pub enum Step<V> {
    Yielded(V),
    Returned(V),
}

macro_rules! guest_fixture {
    (
        pub fn $name:ident<$backend:ident>()
        where $bound:ident: [$first:path $(, $remaining:path)* $(,)?]
        using $builder:expr;
        |$guest:ident| $body:block
    ) => {
        pub fn $name<$backend>()
        where
            $backend: $first $(+ $remaining)*,
        {
            let $guest = $builder
                .build()
                .unwrap()
                .guest()
                .build()
                .unwrap();

            $body
        }
    };
    (
        pub async fn $name:ident<$backend:ident>()
        where $bound:ident: [$first:path $(, $remaining:path)* $(,)?]
        using $builder:expr;
        |$guest:ident| $body:block
    ) => {
        pub async fn $name<$backend>()
        where
            $backend: $first $(+ $remaining)*,
        {
            let $guest = $builder
                .build()
                .unwrap()
                .guest()
                .build()
                .unwrap();

            $body
        }
    };
}

pub(crate) use guest_fixture;

#[doc(hidden)]
pub mod fixtures {
    use std::collections::HashMap;

    use super::{
        Backend,
        BackendCallables,
        BackendClasses,
        BackendCoroutines,
        BackendExceptions,
        BackendInterrupt,
        BackendModules,
        BackendValues,
    };
    use crate::{
        errors::Error,
        handle::Function,
        host::{function::HostFn, module::ModuleSpec},
        runtime::Runtime,
    };

    guest_fixture! {
        pub fn runs_guest_python<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendExceptions,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder();
        |guest| {
            guest
                .exec(
                    r#"
def double(n):
    return n * 2
"#,
                )
                .unwrap();

            assert_eq!(guest.eval::<i64>("double(21)").unwrap(), 42);
        }
    }

    guest_fixture! {
        pub fn calls_a_host_function_passed_as_an_argument<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendExceptions,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder();
        |guest| {
            guest
                .exec(
                    r#"
def invoke(options):
    return options['callback'](21)
"#,
                )
                .unwrap();

            assert_eq!(
                guest
                    .globals()
                    .unwrap()
                    .item::<Function<_>, _>("invoke")
                    .unwrap()
                    .call::<_, i64>((HashMap::from([(
                        "callback",
                        HostFn::new(|enter, args| {
                            Ok::<_, Error>(
                                args.required::<i64>(enter, 0, "n")? * 2,
                            )
                        }),
                    )]),))
                    .unwrap(),
                42,
            );
        }
    }

    guest_fixture! {
        pub fn runs_with_only_synchronous_capability_bounds<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendModules,
        ]
        using Runtime::<B>::builder()
            .bind(
                ModuleSpec::new("sync_host")
                    .constant("answer", 42)
                    .function("double", |enter, args| {
                        Ok::<_, Error>(
                            args.required::<i64>(enter, 0, "value")? * 2,
                        )
                    }),
            );
        |guest| {
            guest
                .exec(
                    r#"
import sync_host
result = sync_host.double(sync_host.answer)
"#,
                )
                .unwrap();

            assert_eq!(guest.eval::<i64>("result").unwrap(), 84);

            guest.close().unwrap();
        }
    }

    #[doc(hidden)]
    #[macro_export]
    macro_rules! __guestpy_backend_tests {
        ($backend:ty) => {
            #[test]
            fn runs_guest_python() {
                $crate::backend::fixtures::runs_guest_python::<$backend>();
            }

            #[test]
            fn calls_a_host_function_passed_as_an_argument() {
                $crate::backend::fixtures::calls_a_host_function_passed_as_an_argument::<
                    $backend,
                >();
            }

            #[test]
            fn runs_with_only_synchronous_capability_bounds() {
                $crate::backend::fixtures::runs_with_only_synchronous_capability_bounds::<
                    $backend,
                >();
            }
        };
    }

    pub use crate::__guestpy_backend_tests as tests;
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{cell::RefCell, collections::VecDeque, rc::Rc};

    use super::{
        Backend, BackendCallables, BackendClasses, BackendCoroutines, BackendExceptions,
        BackendInterrupt, BackendLibrary, NoNativeExtensions, Step, Tok, Val, callables::RawBody,
        modules::BackendModules, values::BackendValues,
    };
    use crate::errors::{Error, GuestException};

    pub(crate) struct Stub;

    #[derive(Clone, Debug, PartialEq)]
    pub(crate) enum StubValue {
        None,
        Bool(bool),
        Int(i64),
        UInt(u64),
        Float(f64),
        Str(String),
        Bytes(Vec<u8>),
        List(Vec<StubValue>),
        Tuple(Vec<StubValue>),
        Dict(Vec<(StubValue, StubValue)>),
        Set(Vec<StubValue>),
        Iterator(Rc<RefCell<VecDeque<StubValue>>>),
    }

    impl Backend for Stub {
        type Engine = ();
        type Context = ();
        type Token<'py> = ();
        type Value<'py> = StubValue;
        type Owned = ();
        type Config = ();
        type NativeExtensions = NoNativeExtensions;

        const NAME: &'static str = "stub";

        fn engine(_: Self::Config) -> Result<Self::Engine, Error> {
            unimplemented!()
        }
        fn shutdown(_: Self::Engine) -> Result<(), Error> {
            unimplemented!()
        }
        fn enter<F, R>(_: &Self::Engine, _: F) -> R
        where
            F: for<'py> FnOnce(Self::Token<'py>) -> R,
        {
            unimplemented!()
        }
        fn new_context<'py>(
            _: Self::Token<'py>,
            _: Self::Value<'py>,
            _: Self::Value<'py>,
        ) -> Self::Context {
            unimplemented!()
        }
        fn context_globals<'py>(_: Self::Token<'py>, _: &Self::Context) -> Self::Value<'py> {
            unimplemented!()
        }
        fn context_builtins<'py>(_: Self::Token<'py>, _: &Self::Context) -> Self::Value<'py> {
            unimplemented!()
        }
        fn detach<'py>(_: Self::Token<'py>, _: Self::Value<'py>) -> Self::Owned {
            unimplemented!()
        }
        fn attach<'py>(_: Self::Token<'py>, _: &Self::Owned) -> Self::Value<'py> {
            unimplemented!()
        }
        fn owned_ptr_eq(_: &Self::Owned, _: &Self::Owned) -> bool {
            unimplemented!()
        }
        fn release(_: Self::Owned) {
            unimplemented!()
        }
    }

    impl BackendValues for Stub {
        fn none<'py>(_: Tok<'py, Self>) -> Val<'py, Self> {
            StubValue::None
        }
        fn bool<'py>(_: Tok<'py, Self>, value: bool) -> Val<'py, Self> {
            StubValue::Bool(value)
        }
        fn int<'py>(_: Tok<'py, Self>, value: i64) -> Val<'py, Self> {
            StubValue::Int(value)
        }
        fn uint<'py>(_: Tok<'py, Self>, value: u64) -> Val<'py, Self> {
            StubValue::UInt(value)
        }
        fn float<'py>(_: Tok<'py, Self>, value: f64) -> Val<'py, Self> {
            StubValue::Float(value)
        }
        fn str<'py>(_: Tok<'py, Self>, value: &str) -> Val<'py, Self> {
            StubValue::Str(value.to_owned())
        }
        fn bytes<'py>(_: Tok<'py, Self>, value: &[u8]) -> Val<'py, Self> {
            StubValue::Bytes(value.to_owned())
        }
        fn list<'py>(
            _: Tok<'py, Self>,
            items: Vec<Val<'py, Self>>,
        ) -> Result<Val<'py, Self>, Error> {
            Ok(StubValue::List(items))
        }
        fn tuple<'py>(
            _: Tok<'py, Self>,
            items: Vec<Val<'py, Self>>,
        ) -> Result<Val<'py, Self>, Error> {
            Ok(StubValue::Tuple(items))
        }
        fn dict<'py>(
            _: Tok<'py, Self>,
            pairs: Vec<(Val<'py, Self>, Val<'py, Self>)>,
        ) -> Result<Val<'py, Self>, Error> {
            Ok(StubValue::Dict(pairs))
        }
        fn set<'py>(
            _: Tok<'py, Self>,
            items: Vec<Val<'py, Self>>,
        ) -> Result<Val<'py, Self>, Error> {
            Ok(StubValue::Set(items))
        }
        fn new_dict<'py>(_: Tok<'py, Self>) -> Result<Val<'py, Self>, Error> {
            Ok(StubValue::Dict(Vec::new()))
        }
        fn is_bool<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
            matches!(value, StubValue::Bool(_))
        }
        fn is_int<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
            matches!(value, StubValue::Int(_) | StubValue::UInt(_))
        }
        fn is_float<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
            matches!(value, StubValue::Float(_))
        }
        fn is_str<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
            matches!(value, StubValue::Str(_))
        }
        fn is_bytes<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
            matches!(value, StubValue::Bytes(_))
        }
        fn is_list<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
            matches!(value, StubValue::List(_))
        }
        fn is_tuple<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
            matches!(value, StubValue::Tuple(_))
        }
        fn is_dict<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
            matches!(value, StubValue::Dict(_))
        }
        fn is_set<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
            matches!(value, StubValue::Set(_))
        }
        fn is_callable<'py>(_: Tok<'py, Self>, _: &Val<'py, Self>) -> bool {
            unimplemented!()
        }
        fn is_class<'py>(_: Tok<'py, Self>, _: &Val<'py, Self>) -> bool {
            unimplemented!()
        }
        fn is_instance<'py>(
            _: Tok<'py, Self>,
            _: &Val<'py, Self>,
            _: &Val<'py, Self>,
        ) -> Result<bool, Error> {
            unimplemented!()
        }
        fn is_subclass<'py>(
            _: Tok<'py, Self>,
            _: &Val<'py, Self>,
            _: &Val<'py, Self>,
        ) -> Result<bool, Error> {
            unimplemented!()
        }
        fn is_iterable<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
            matches!(
                value,
                StubValue::List(_) | StubValue::Tuple(_) | StubValue::Dict(_) | StubValue::Set(_)
            )
        }
        fn is_none<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
            matches!(value, StubValue::None)
        }
        fn as_bool<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<bool, Error> {
            match value {
                StubValue::Bool(value) => Ok(*value),
                other => Err(Error::type_mismatch("bool", &Self::type_name((), other))),
            }
        }
        fn as_i64<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<i64, Error> {
            match value {
                StubValue::Int(value) => Ok(*value),
                StubValue::UInt(value) => (*value)
                    .try_into()
                    .map_err(|_| Error::conversion("value does not fit in i64")),
                other => Err(Error::type_mismatch("int", &Self::type_name((), other))),
            }
        }
        fn as_u64<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<u64, Error> {
            match value {
                StubValue::UInt(value) => Ok(*value),
                StubValue::Int(value) => (*value)
                    .try_into()
                    .map_err(|_| Error::conversion("value does not fit in u64")),
                other => Err(Error::type_mismatch("int", &Self::type_name((), other))),
            }
        }
        fn as_f64<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<f64, Error> {
            match value {
                StubValue::Float(value) => Ok(*value),
                other => Err(Error::type_mismatch("float", &Self::type_name((), other))),
            }
        }
        fn as_str<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<String, Error> {
            match value {
                StubValue::Str(value) => Ok(value.clone()),
                other => Err(Error::type_mismatch("str", &Self::type_name((), other))),
            }
        }
        fn as_bytes<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<Vec<u8>, Error> {
            match value {
                StubValue::Bytes(value) => Ok(value.clone()),
                other => Err(Error::type_mismatch("bytes", &Self::type_name((), other))),
            }
        }
        fn len<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<usize, Error> {
            match value {
                StubValue::List(items) | StubValue::Tuple(items) | StubValue::Set(items) => {
                    Ok(items.len())
                }
                StubValue::Dict(pairs) => Ok(pairs.len()),
                other => {
                    Err(Error::type_mismatch("a sized container", &Self::type_name((), other)))
                }
            }
        }
        fn type_name<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> String {
            match value {
                StubValue::None => "NoneType",
                StubValue::Bool(_) => "bool",
                StubValue::Int(_) | StubValue::UInt(_) => "int",
                StubValue::Float(_) => "float",
                StubValue::Str(_) => "str",
                StubValue::Bytes(_) => "bytes",
                StubValue::List(_) => "list",
                StubValue::Tuple(_) => "tuple",
                StubValue::Dict(_) => "dict",
                StubValue::Set(_) => "set",
                StubValue::Iterator(_) => "iterator",
            }
            .to_owned()
        }
        fn identity<'py>(_: Tok<'py, Self>, _: &Val<'py, Self>) -> usize {
            unimplemented!()
        }
        fn truthy<'py>(_: Tok<'py, Self>, _: &Val<'py, Self>) -> Result<bool, Error> {
            unimplemented!()
        }
        fn repr<'py>(_: Tok<'py, Self>, _: &Val<'py, Self>) -> Result<String, Error> {
            unimplemented!()
        }
        fn display<'py>(_: Tok<'py, Self>, _: &Val<'py, Self>) -> Result<String, Error> {
            unimplemented!()
        }
        fn dir<'py>(_: Tok<'py, Self>, _: &Val<'py, Self>) -> Result<Vec<String>, Error> {
            unimplemented!()
        }
        fn get_attr<'py>(
            _: Tok<'py, Self>,
            _: &Val<'py, Self>,
            _: &str,
        ) -> Result<Val<'py, Self>, Error> {
            unimplemented!()
        }
        fn set_attr<'py>(
            _: Tok<'py, Self>,
            _: &Val<'py, Self>,
            _: &str,
            _: Val<'py, Self>,
        ) -> Result<(), Error> {
            unimplemented!()
        }
        fn del_attr<'py>(_: Tok<'py, Self>, _: &Val<'py, Self>, _: &str) -> Result<(), Error> {
            unimplemented!()
        }
        fn has_attr<'py>(_: Tok<'py, Self>, _: &Val<'py, Self>, _: &str) -> bool {
            unimplemented!()
        }
        fn get_item<'py>(
            _: Tok<'py, Self>,
            value: &Val<'py, Self>,
            key: &Val<'py, Self>,
        ) -> Result<Val<'py, Self>, Error> {
            match value {
                StubValue::Dict(pairs) => pairs
                    .iter()
                    .find(|(candidate, _)| candidate == key)
                    .map(|(_, value)| value.clone())
                    .ok_or_else(|| Error::conversion("key not found")),
                other => Err(Error::type_mismatch("dict", &Self::type_name((), other))),
            }
        }
        fn get_item_opt<'py>(
            _: Tok<'py, Self>,
            value: &Val<'py, Self>,
            key: &Val<'py, Self>,
        ) -> Result<Option<Val<'py, Self>>, Error> {
            match value {
                StubValue::Dict(pairs) => Ok(pairs
                    .iter()
                    .find(|(candidate, _)| candidate == key)
                    .map(|(_, value)| value.clone())),
                other => Err(Error::type_mismatch("dict", &Self::type_name((), other))),
            }
        }
        fn set_item<'py>(
            _: Tok<'py, Self>,
            _: &Val<'py, Self>,
            _: Val<'py, Self>,
            _: Val<'py, Self>,
        ) -> Result<(), Error> {
            unimplemented!()
        }
        fn del_item<'py>(
            _: Tok<'py, Self>,
            _: &Val<'py, Self>,
            _: &Val<'py, Self>,
        ) -> Result<(), Error> {
            unimplemented!()
        }
        fn copy_dict<'py>(_: Tok<'py, Self>, _: &Val<'py, Self>) -> Result<Val<'py, Self>, Error> {
            unimplemented!()
        }
        fn call<'py>(
            _: Tok<'py, Self>,
            _: &Val<'py, Self>,
            _: &[Val<'py, Self>],
            _: &[(&str, Val<'py, Self>)],
        ) -> Result<Val<'py, Self>, Error> {
            unimplemented!()
        }
        fn iter<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<Val<'py, Self>, Error> {
            let items = match value {
                StubValue::List(items) | StubValue::Tuple(items) | StubValue::Set(items) => {
                    items.clone()
                }
                StubValue::Dict(pairs) => pairs
                    .iter()
                    .map(|(key, _)| key.clone())
                    .collect(),
                other => {
                    return Err(Error::type_mismatch("an iterable", &Self::type_name((), other)));
                }
            };

            Ok(StubValue::Iterator(Rc::new(RefCell::new(VecDeque::from(items)))))
        }
        fn next<'py>(
            _: Tok<'py, Self>,
            iterator: &Val<'py, Self>,
        ) -> Result<Option<Val<'py, Self>>, Error> {
            match iterator {
                StubValue::Iterator(remaining) => Ok(remaining.borrow_mut().pop_front()),
                other => Err(Error::type_mismatch("an iterator", &Self::type_name((), other))),
            }
        }
        fn send<'py>(
            _: Tok<'py, Self>,
            _: &Val<'py, Self>,
            _: Val<'py, Self>,
        ) -> Result<Step<Val<'py, Self>>, Error> {
            unimplemented!()
        }
        fn throw<'py>(
            _: Tok<'py, Self>,
            _: &Val<'py, Self>,
            _: Val<'py, Self>,
        ) -> Result<Step<Val<'py, Self>>, Error> {
            unimplemented!()
        }
        fn close<'py>(_: Tok<'py, Self>, _: &Val<'py, Self>) -> Result<(), Error> {
            unimplemented!()
        }
    }

    impl BackendExceptions for Stub {
        type Raw = ();

        fn take_error<'py>(_: Tok<'py, Self>, _: Self::Raw) -> GuestException {
            unimplemented!()
        }
        fn raise<'py>(_: Tok<'py, Self>, _: Error) -> Self::Raw {
            unimplemented!()
        }
        fn exception_object<'py>(_: Tok<'py, Self>, _: Error) -> Result<Val<'py, Self>, Error> {
            unimplemented!()
        }
        fn exception_class<'py>(_: Tok<'py, Self>, _: &str) -> Result<Val<'py, Self>, Error> {
            unimplemented!()
        }
        fn new_exception_class<'py>(
            _: Tok<'py, Self>,
            _: &str,
            _: &str,
            _: Option<&Val<'py, Self>>,
        ) -> Result<Val<'py, Self>, Error> {
            unimplemented!()
        }
    }

    impl BackendCallables for Stub {
        fn function<'py>(
            _: Tok<'py, Self>,
            _: &str,
            _: Option<&str>,
            _: RawBody<Self>,
        ) -> Result<Val<'py, Self>, Error> {
            unimplemented!()
        }
        fn method<'py>(
            _: Tok<'py, Self>,
            _: &str,
            _: Option<&str>,
            _: RawBody<Self>,
        ) -> Result<Val<'py, Self>, Error> {
            unimplemented!()
        }
    }

    impl BackendModules for Stub {
        fn new_module<'py>(
            _: Tok<'py, Self>,
            _: &str,
            _: Val<'py, Self>,
            _: Option<&str>,
        ) -> Result<Val<'py, Self>, Error> {
            unimplemented!()
        }
        fn compile<'py>(_: Tok<'py, Self>, _: &str, _: &str) -> Result<Val<'py, Self>, Error> {
            unimplemented!()
        }
        fn exec_code<'py>(
            _: Tok<'py, Self>,
            _: &Val<'py, Self>,
            _: &Val<'py, Self>,
        ) -> Result<(), Error> {
            unimplemented!()
        }
        fn eval<'py>(
            _: Tok<'py, Self>,
            _: &str,
            _: &str,
            _: &Val<'py, Self>,
        ) -> Result<Val<'py, Self>, Error> {
            unimplemented!()
        }
        fn builtins_dict<'py>(_: Tok<'py, Self>) -> Result<Val<'py, Self>, Error> {
            unimplemented!()
        }
        fn install_dispatcher<'py>(
            _: Tok<'py, Self>,
            _: &Self::Engine,
            _: Val<'py, Self>,
        ) -> Result<(), Error> {
            unimplemented!()
        }
        fn real_import<'py>(_: Tok<'py, Self>) -> Result<Val<'py, Self>, Error> {
            unimplemented!()
        }
    }

    impl BackendLibrary for Stub {
        type NativeModule = StubValue;

        fn declare_native<'py>(
            _: Tok<'py, Self>,
            native: &Self::NativeModule,
            _: &str,
        ) -> Result<Val<'py, Self>, Error> {
            Ok(native.clone())
        }
    }

    impl BackendClasses for Stub {
        type Ref<'a, C: 'static> = &'a C;
        type RefMut<'a, C: 'static> = &'a mut C;

        fn new_class<'py>(
            _: Tok<'py, Self>,
            _: &str,
            _: &[Val<'py, Self>],
            _: &Val<'py, Self>,
        ) -> Result<Val<'py, Self>, Error> {
            unimplemented!()
        }
        fn alloc<'py, C: 'static>(
            _: Tok<'py, Self>,
            _: &Val<'py, Self>,
        ) -> Result<Val<'py, Self>, Error> {
            unimplemented!()
        }
        fn set_payload<'py, C: 'static>(
            _: Tok<'py, Self>,
            _: &Val<'py, Self>,
            _: C,
        ) -> Result<(), Error> {
            unimplemented!()
        }
        fn borrow<'py, 'a, C: 'static>(
            _: Tok<'py, Self>,
            _: &'a Val<'py, Self>,
        ) -> Result<Self::Ref<'a, C>, Error> {
            unimplemented!()
        }
        fn borrow_mut<'py, 'a, C: 'static>(
            _: Tok<'py, Self>,
            _: &'a Val<'py, Self>,
        ) -> Result<Self::RefMut<'a, C>, Error> {
            unimplemented!()
        }
        fn is_host_instance<'py>(_: Tok<'py, Self>, _: &Val<'py, Self>) -> bool {
            unimplemented!()
        }
    }

    impl BackendCoroutines for Stub {
        fn is_coroutine<'py>(_: Tok<'py, Self>, _: &Val<'py, Self>) -> bool {
            unimplemented!()
        }
        fn is_awaitable<'py>(_: Tok<'py, Self>, _: &Val<'py, Self>) -> bool {
            unimplemented!()
        }
        fn anext<'py>(_: Tok<'py, Self>, _: &Val<'py, Self>) -> Result<Val<'py, Self>, Error> {
            unimplemented!()
        }
        fn asend<'py>(
            _: Tok<'py, Self>,
            _: &Val<'py, Self>,
            _: Val<'py, Self>,
        ) -> Result<Val<'py, Self>, Error> {
            unimplemented!()
        }
        fn athrow<'py>(
            _: Tok<'py, Self>,
            _: &Val<'py, Self>,
            _: Val<'py, Self>,
        ) -> Result<Val<'py, Self>, Error> {
            unimplemented!()
        }
        fn aclose<'py>(_: Tok<'py, Self>, _: &Val<'py, Self>) -> Result<Val<'py, Self>, Error> {
            unimplemented!()
        }
        fn set_running_loop<'py>(
            _: Tok<'py, Self>,
            _: Option<&Val<'py, Self>>,
        ) -> Result<(), Error> {
            unimplemented!()
        }
    }

    impl BackendInterrupt for Stub {
        type Handle = ();

        fn handle(_: &Self::Engine) -> Self::Handle {
            unimplemented!()
        }
        fn request(_: &Self::Handle) {
            unimplemented!()
        }
        fn check<'py>(_: Tok<'py, Self>) -> Result<(), Error> {
            unimplemented!()
        }
        fn reset(_: &Self::Engine) {
            unimplemented!()
        }
    }
}
