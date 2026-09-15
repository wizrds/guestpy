use std::ops::{Deref, DerefMut};

use super::{Backend, BackendCallables, BackendValues, Tok, Val};
use crate::errors::Error;

pub trait BackendClasses: Backend + BackendValues + BackendCallables {
    type Ref<'a, C: 'static>: Deref<Target = C> + 'a
    where
        Self: 'a;

    type RefMut<'a, C: 'static>: DerefMut<Target = C> + 'a
    where
        Self: 'a;

    fn native_base<'py>(token: Tok<'py, Self>) -> Val<'py, Self>;

    fn alloc<'py, C: 'static>(
        token: Tok<'py, Self>,
        class: &Val<'py, Self>,
    ) -> Result<Val<'py, Self>, Error>;

    fn set_payload<'py, C: 'static>(
        token: Tok<'py, Self>,
        instance: &Val<'py, Self>,
        payload: C,
    ) -> Result<(), Error>;

    fn instantiate<'py, C: 'static>(
        token: Tok<'py, Self>,
        class: &Val<'py, Self>,
        payload: C,
    ) -> Result<Val<'py, Self>, Error> {
        let instance = Self::alloc::<C>(token, class)?;

        Self::set_payload::<C>(token, &instance, payload)?;

        Ok(instance)
    }

    fn borrow<'py, 'a, C: 'static>(
        token: Tok<'py, Self>,
        instance: &'a Val<'py, Self>,
    ) -> Result<Self::Ref<'a, C>, Error>;

    fn borrow_mut<'py, 'a, C: 'static>(
        token: Tok<'py, Self>,
        instance: &'a Val<'py, Self>,
    ) -> Result<Self::RefMut<'a, C>, Error>;

    fn is_host_instance<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> bool;

    fn generic_alias<'py>(
        token: Tok<'py, Self>,
        origin: &Val<'py, Self>,
        arguments: &[Val<'py, Self>],
    ) -> Result<Val<'py, Self>, Error>;
}

#[doc(hidden)]
pub mod fixtures {
    use std::{cell::Cell, collections::HashMap};

    use crate::{
        backend::{
            Backend, BackendCallables, BackendClasses, BackendCoroutines, BackendExceptions,
            BackendInterrupt, BackendModules, BackendValues, guest_fixture,
        },
        errors::Error,
        handle::{
            Annotated, AsyncIter, Class, Coroutine, Function, GenericAlias, Instance, Module,
            Named, Object, ObjectProtocol, TypeProtocol,
        },
        host::{
            class::{ClassBuilder, HostClass, HostClassDefinition},
            dunder::Dunder,
            exception::{ExceptionClass, Raise},
            iter::HostIter,
            module::ModuleSpec,
        },
        marshal::ToGuest,
        runtime::Runtime,
    };

    struct Vector2 {
        x: f64,
        y: f64,
    }

    impl HostClass for Vector2 {
        const NAME: &'static str = "Vector2";
    }

    impl<B> HostClassDefinition<B> for Vector2
    where
        B: Backend + BackendValues + BackendCallables + BackendClasses,
    {
        fn build(builder: &mut ClassBuilder<B, Self>) {
            builder
                .constructor(|enter, args| {
                    let x = args.required::<f64>(enter, 0, "x")?;
                    let y = args.required::<f64>(enter, 1, "y")?;

                    args.finish()?;

                    Ok(Self { x, y })
                })
                .method("length", |receiver, _, _| {
                    let vector = receiver.payload::<Self>()?;

                    Ok::<_, Error>(vector.x.hypot(vector.y))
                })
                .getter("x", |receiver, _| Ok::<_, Error>(receiver.payload::<Self>()?.x))
                .setter("x", |receiver, _, value: f64| {
                    receiver.payload_mut::<Self>()?.x = value;

                    Ok::<_, Error>(())
                });
        }
    }

    struct Contract;

    impl HostClass for Contract {
        const NAME: &'static str = "Contract";
        const DOC: Option<&'static str> = Some("Reports a result for an input.");
    }

    impl<B> HostClassDefinition<B> for Contract
    where
        B: Backend
            + BackendValues
            + BackendCallables
            + BackendClasses
            + BackendModules
            + BackendCoroutines,
    {
        fn build(builder: &mut ClassBuilder<B, Self>) {
            builder
                .constructor(|_, args| {
                    args.finish()?;

                    Ok(Self)
                })
                .generic()
                .async_method("invoke", |receiver, enter, args| {
                    let city = args.required::<String>(enter, 0, "city")?;

                    args.finish()?;

                    let this = receiver.resolve::<Object<B>>()?;

                    Ok(async move { this.call_method::<_, String>("execute", (city,)) })
                });
        }
    }

    struct Ledger {
        prefix: String,
    }

    impl HostClass for Ledger {
        const NAME: &'static str = "Ledger";
    }

    impl<B> HostClassDefinition<B> for Ledger
    where
        B: Backend
            + BackendValues
            + BackendCallables
            + BackendClasses
            + BackendModules
            + BackendCoroutines,
    {
        fn build(builder: &mut ClassBuilder<B, Self>) {
            builder
                .constructor(|enter, args| {
                    let prefix = args.required::<String>(enter, 0, "prefix")?;

                    args.finish()?;

                    Ok(Self { prefix })
                })
                .method("describe", |receiver, _, args| {
                    args.finish()?;

                    let this = receiver.resolve::<Object<B>>()?;
                    let ledger = receiver.payload::<Self>()?;

                    Ok::<_, Error>(format!(
                        "{}/{}",
                        ledger.prefix,
                        this.call_method::<_, String>("label", ())?,
                    ))
                })
                .async_method("describe_later", |receiver, _, args| {
                    args.finish()?;

                    let this = receiver.resolve::<Object<B>>()?;
                    let prefix = receiver
                        .payload::<Self>()?
                        .prefix
                        .clone();

                    Ok::<_, Error>(async move {
                        Ok(format!("{}/{}", prefix, this.call_method::<_, String>("label", ())?,))
                    })
                });
        }
    }

    struct HostMapping {
        entries: HashMap<String, i64>,
    }

    impl HostClass for HostMapping {
        const NAME: &'static str = "HostMapping";
    }

    impl<B> HostClassDefinition<B> for HostMapping
    where
        B: Backend + BackendValues + BackendCallables + BackendClasses + BackendModules,
    {
        fn build(builder: &mut ClassBuilder<B, Self>) {
            builder
                .constructor(|_, args| {
                    args.finish()?;

                    Ok(Self {
                        entries: [(String::from("answer"), 42)]
                            .into_iter()
                            .collect(),
                    })
                })
                .imported_base("collections.abc", "Mapping")
                .method(Dunder::GetItem, |receiver, enter, args| {
                    let key = args.required::<String>(enter, 0, "key")?;

                    args.finish()?;

                    let mapping = receiver.payload::<Self>()?;

                    mapping
                        .entries
                        .get(&key)
                        .copied()
                        .ok_or_else(|| {
                            Error::from(
                                Raise::<B>::new(ExceptionClass::builtin("KeyError")).arg(key),
                            )
                        })
                })
                .method(Dunder::Iter, |receiver, _, args| {
                    args.finish()?;

                    let mapping = receiver.payload::<Self>()?;

                    Ok::<_, Error>(HostIter::new(
                        mapping
                            .entries
                            .keys()
                            .cloned()
                            .map(Ok)
                            .collect::<Vec<_>>()
                            .into_iter(),
                    ))
                })
                .method(Dunder::Len, |receiver, _, args| {
                    args.finish()?;

                    Ok::<_, Error>(
                        receiver
                            .payload::<Self>()?
                            .entries
                            .len(),
                    )
                })
                .generic();
        }
    }

    struct AbstractMapping;

    impl HostClass for AbstractMapping {
        const NAME: &'static str = "AbstractMapping";
    }

    impl<B> HostClassDefinition<B> for AbstractMapping
    where
        B: Backend + BackendValues + BackendCallables + BackendClasses + BackendModules,
    {
        fn build(builder: &mut ClassBuilder<B, Self>) {
            builder
                .constructor(|_, args| {
                    args.finish()?;

                    Ok(Self)
                })
                .imported_base("collections.abc", "Mapping")
                .method(Dunder::GetItem, |_, _, args| {
                    args.finish()?;

                    Ok::<_, Error>(42_i64)
                })
                .method(Dunder::Iter, |_, _, args| {
                    args.finish()?;

                    Ok::<_, Error>(HostIter::new(vec![Ok(String::from("answer"))].into_iter()))
                });
        }
    }

    struct ConcreteMapping;

    impl HostClass for ConcreteMapping {
        const NAME: &'static str = "ConcreteMapping";
    }

    impl<B> HostClassDefinition<B> for ConcreteMapping
    where
        B: Backend + BackendValues + BackendCallables + BackendClasses + BackendModules,
    {
        fn build(builder: &mut ClassBuilder<B, Self>) {
            builder
                .constructor(|_, args| {
                    args.finish()?;

                    Ok(Self)
                })
                .base::<AbstractMapping>()
                .method(Dunder::Len, |_, _, args| {
                    args.finish()?;

                    Ok::<_, Error>(1_i64)
                });
        }
    }

    struct MappingParent;

    impl HostClass for MappingParent {
        const NAME: &'static str = "MappingParent";
    }

    impl<B> HostClassDefinition<B> for MappingParent
    where
        B: Backend + BackendValues + BackendCallables + BackendClasses,
    {
        fn build(_: &mut ClassBuilder<B, Self>) {}
    }

    struct MixedMapping;

    impl HostClass for MixedMapping {
        const NAME: &'static str = "MixedMapping";
    }

    impl<B> HostClassDefinition<B> for MixedMapping
    where
        B: Backend + BackendValues + BackendCallables + BackendClasses + BackendModules,
    {
        fn build(builder: &mut ClassBuilder<B, Self>) {
            builder
                .base::<MappingParent>()
                .imported_base("collections.abc", "Mapping");
        }
    }

    guest_fixture! {
        pub fn host_borrows_dynamic_and_typed_payloads<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("geometry").class::<Vector2>().expect("Vector2 registers cleanly"));
        |guest| {
            guest.exec("import geometry").unwrap();

            let dynamic = guest
                .eval::<Instance<B>>("geometry.Vector2(3, 4)")
                .unwrap();

            assert_eq!(
                dynamic
                    .borrow_as_with::<Vector2, _, _>(|vector| vector.x)
                    .unwrap(),
                3.0,
            );

            let typed = dynamic.as_typed::<Vector2>().unwrap();

            typed
                .borrow_with_mut(|vector| {
                    vector.x = 1.0;
                })
                .unwrap();

            assert_eq!(typed.borrow_with(|vector| vector.x).unwrap(), 1.0);
        }
    }

    guest_fixture! {
        pub fn ordinary_guest_class_uses_instance_handle<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("geometry").class::<Vector2>().expect("Vector2 registers cleanly"));
        |guest| {
            guest.exec("import geometry").unwrap();
            guest
                .exec(
                    r#"
class GuestVector:
    def __init__(self, x):
        self.x = x

value = GuestVector(3)
"#,
                )
                .unwrap();

            assert_eq!(
                guest
                    .globals()
                    .unwrap()
                    .item::<Instance<_>, _>("value")
                    .unwrap()
                    .get::<i64>("x")
                    .unwrap(),
                3,
            );
        }
    }

    guest_fixture! {
        pub fn class_handle_rejects_other_callables<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("geometry").class::<Vector2>().expect("Vector2 registers cleanly"));
        |guest| {
            guest.exec("import geometry").unwrap();
            guest
                .exec(
                    r#"
def callable_value():
    pass
"#,
                )
                .unwrap();

            assert!(guest.eval::<Class<_>>("geometry.Vector2").is_ok());
            assert!(guest.eval::<Class<_>>("lambda: None").is_err());
            assert!(
                guest
                    .globals()
                    .unwrap()
                    .class("callable_value")
                    .is_err(),
            );
        }
    }

    guest_fixture! {
        pub fn class_constructs_and_overrides_its_result_descriptor<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("geometry").class::<Vector2>().expect("Vector2 registers cleanly"));
        |guest| {
            guest.exec("import geometry").unwrap();

            let class = guest
                .eval::<Class<_, Vector2>>("geometry.Vector2")
                .unwrap();
            let typed = class.construct((3.0_f64, 4.0_f64)).unwrap();
            let dynamic = class
                .construct_as::<_, Instance<_>>((6.0_f64, 8.0_f64))
                .unwrap();
            let retyped = class.with_result::<Instance<_>>();

            assert_eq!(
                typed
                    .borrow_with(|vector| vector.x.hypot(vector.y))
                    .unwrap(),
                5.0,
            );
            assert_eq!(dynamic.get::<f64>("x").unwrap(), 6.0);
            assert_eq!(
                retyped
                    .construct((5.0_f64, 12.0_f64))
                    .unwrap()
                    .borrow_as_with::<Vector2, _, _>(|vector| vector.y)
                    .unwrap(),
                12.0,
            );
            assert!(class.value().ptr_eq(&retyped.value()));
        }
    }

    guest_fixture! {
        pub fn guest_subclass_works<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("geometry").class::<Vector2>().expect("Vector2 registers cleanly"));
        |guest| {
            guest.exec("import geometry").unwrap();
            guest
                .exec(
                    r#"
class Tagged(geometry.Vector2):
    def __init__(self, x, y):
        super().__init__(x, y)
        self.tag = 'tagged'

t = Tagged(3, 4)
"#,
                )
                .unwrap();

            assert_eq!(
                guest
                    .eval::<Vector2>("t")
                    .unwrap()
                    .borrow_with(|vector| vector.x.hypot(vector.y))
                    .unwrap(),
                5.0,
            );
        }
    }

    guest_fixture! {
        pub fn subclass_that_skips_super_init_fails_clearly<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("geometry").class::<Vector2>().expect("Vector2 registers cleanly"));
        |guest| {
            guest.exec("import geometry").unwrap();
            guest
                .exec(
                    r#"
class Empty(geometry.Vector2):
    def __init__(self, x, y):
        pass

e = Empty(3, 4)
"#,
                )
                .unwrap();

            assert!(
                guest
                    .eval::<Vector2>("e")
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("no payload"),
            );
        }
    }

    guest_fixture! {
        pub fn class_handle_reads_class_attributes<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder();
        |guest| {
            guest
                .exec(
                    r#"
class Impl:
    description = 'a description'
"#,
                )
                .unwrap();

            let class = guest.eval::<Class<B>>("Impl").unwrap();

            assert_eq!(class.get::<String>("description").unwrap(), "a description");
            assert!(class.has("description").unwrap());
            assert!(!class.has("missing").unwrap());
            assert!(class.dir().unwrap().contains(&String::from("description")));
            assert_eq!(class.name().unwrap(), "Impl");
        }
    }

    guest_fixture! {
        pub fn module_and_function_expose_names_and_annotations<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder();
        |guest| {
            guest
                .exec(
                    r#"
import types

m = types.ModuleType('probe')
m.__doc__ = 'a probe'

def scale(value: int, factor: float) -> float:
    return value * factor
"#,
                )
                .unwrap();

            let module = guest.eval::<Module<B>>("m").unwrap();
            let function = guest.eval::<Function<B>>("scale").unwrap();

            assert_eq!(module.name().unwrap(), "probe");
            assert_eq!(module.doc().unwrap(), Some(String::from("a probe")));
            assert_eq!(function.name().unwrap(), "scale");
            assert!(function.annotation("factor").unwrap().is_some());
            assert!(function.annotation("missing").unwrap().is_none());
            assert_eq!(
                function
                    .annotations()
                    .unwrap()
                    .iter()
                    .map(|(name, _)| name.clone())
                    .collect::<Vec<_>>(),
                vec![
                    String::from("value"),
                    String::from("factor"),
                    String::from("return"),
                ],
            );
        }
    }

    guest_fixture! {
        pub fn class_reports_bases_and_mro<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder();
        |guest| {
            guest
                .exec(
                    r#"
class Base:
    pass

class Derived(Base):
    pass
"#,
                )
                .unwrap();

            let derived = guest.eval::<Class<B>>("Derived").unwrap();

            assert_eq!(
                derived
                    .bases()
                    .unwrap()
                    .iter()
                    .map(Named::name)
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap(),
                vec![String::from("Base")],
            );
            assert_eq!(
                derived
                    .mro()
                    .unwrap()
                    .iter()
                    .map(Named::name)
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap(),
                vec![
                    String::from("Derived"),
                    String::from("Base"),
                    String::from("object"),
                ],
            );
        }
    }

    guest_fixture! {
        pub fn isinstance_and_issubclass_agree_across_backends<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("host_lib").class::<Contract>().expect("Contract registers cleanly"));
        |guest| {
            guest.exec("import host_lib").unwrap();
            guest
                .exec(
                    r#"
class Impl(host_lib.Contract):
    pass

class Plain:
    pass

i = Impl()
p = Plain()
"#,
                )
                .unwrap();

            let contract = guest.eval::<Class<B>>("host_lib.Contract").unwrap();
            let implementation = guest.eval::<Class<B>>("Impl").unwrap();
            let plain = guest.eval::<Class<B>>("Plain").unwrap();
            let instance = guest.eval::<Object<B>>("i").unwrap();
            let other = guest.eval::<Object<B>>("p").unwrap();

            assert!(instance.is_instance_of(&contract).unwrap());
            assert!(instance.is_instance_of(&implementation).unwrap());
            assert!(!other.is_instance_of(&contract).unwrap());
            assert!(implementation.is_subclass_of(&contract).unwrap());
            assert!(!plain.is_subclass_of(&contract).unwrap());
            assert!(contract.is_subclass_of(&contract).unwrap());
        }
    }

    guest_fixture! {
        pub fn host_class_is_generic<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("host_lib").class::<Contract>().expect("Contract registers cleanly"));
        |guest| {
            guest.exec("import host_lib").unwrap();
            guest
                .exec(
                    r#"
class Args:
    pass

class Result:
    pass

alias = host_lib.Contract[Args, Result]
"#,
                )
                .unwrap();

            let alias = GenericAlias::of(&guest.eval::<Object<B>>("alias").unwrap())
                .unwrap()
                .unwrap();

            assert_eq!(alias.origin().unwrap().name().unwrap(), "Contract");
            assert_eq!(
                alias
                    .arguments()
                    .unwrap()
                    .iter()
                    .map(|argument| {
                        argument
                            .cast::<Class<B>>()
                            .and_then(|class| class.name())
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap(),
                vec![String::from("Args"), String::from("Result")],
            );

            guest.exec("single = host_lib.Contract[Args]").unwrap();

            assert_eq!(
                GenericAlias::of(&guest.eval::<Object<B>>("single").unwrap())
                    .unwrap()
                    .unwrap()
                    .arguments()
                    .unwrap()
                    .len(),
                1,
            );

            guest
                .exec(
                    r#"
class Impl(host_lib.Contract[Args, Result]):
    pass
"#,
                )
                .unwrap();

            let implementation = guest.eval::<Class<B>>("Impl").unwrap();

            assert!(
                implementation
                    .is_subclass_of(&guest.eval::<Class<B>>("host_lib.Contract").unwrap())
                    .unwrap(),
            );
            assert_eq!(
                implementation
                    .generic_base_of(&guest.eval::<Class<B>>("host_lib.Contract").unwrap())
                    .unwrap()
                    .unwrap()
                    .arguments()
                    .unwrap()
                    .len(),
                2,
            );
        }
    }

    guest_fixture! {
        pub async fn host_base_method_calls_guest_override<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("host_lib").class::<Contract>().expect("Contract registers cleanly"));
        |guest| {
            guest.exec("import host_lib").unwrap();
            guest
                .exec(
                    r#"
class Impl(host_lib.Contract):
    def execute(self, city):
        return 'sunny in ' + city
"#,
                )
                .unwrap();

            assert_eq!(
                guest
                    .eval::<Instance<B>>("Impl()")
                    .unwrap()
                    .call_method::<_, Coroutine<B, String>>("invoke", ("Vancouver",))
                    .unwrap()
                    .await
                    .unwrap(),
                "sunny in Vancouver",
            );
        }
    }

    guest_fixture! {
        pub fn paired_method_reads_payload_and_calls_override<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("host_lib").class::<Ledger>().expect("Ledger registers cleanly"));
        |guest| {
            guest.exec("import host_lib").unwrap();
            guest
                .exec(
                    r#"
class Detailed(host_lib.Ledger):
    def label(self):
        return 'detailed'
"#,
                )
                .unwrap();

            assert_eq!(
                guest
                    .eval::<Instance<B>>("Detailed('ledger')")
                    .unwrap()
                    .call_method::<_, String>("describe", ())
                    .unwrap(),
                "ledger/detailed",
            );
        }
    }

    guest_fixture! {
        pub async fn paired_async_method_reads_payload_and_calls_override<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("host_lib").class::<Ledger>().expect("Ledger registers cleanly"));
        |guest| {
            guest.exec("import host_lib").unwrap();
            guest
                .exec(
                    r#"
class Detailed(host_lib.Ledger):
    def label(self):
        return 'detailed'
"#,
                )
                .unwrap();

            assert_eq!(
                guest
                    .eval::<Instance<B>>("Detailed('ledger')")
                    .unwrap()
                    .call_method::<_, Coroutine<B, String>>("describe_later", ())
                    .unwrap()
                    .await
                    .unwrap(),
                "ledger/detailed",
            );
        }
    }

    guest_fixture! {
        pub fn annotations_preserve_declaration_order<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder();
        |guest| {
            guest
                .exec(
                    r#"
class Row:
    city: str
    units: str
    temperature: float
"#,
                )
                .unwrap();

            assert_eq!(
                guest
                    .eval::<Class<B>>("Row")
                    .unwrap()
                    .annotations()
                    .unwrap()
                    .iter()
                    .map(|(name, _)| name.clone())
                    .collect::<Vec<_>>(),
                vec![
                    String::from("city"),
                    String::from("units"),
                    String::from("temperature"),
                ],
            );
        }
    }

    guest_fixture! {
        pub fn any_handle_calls_a_callable_and_reports_a_clear_error_otherwise<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("host_lib").class::<Contract>().expect("Contract registers cleanly"));
        |guest| {
            guest
                .exec(
                    r#"
def twice(value):
    return value * 2
"#,
                )
                .unwrap();

            assert_eq!(
                guest
                    .eval::<Object<B>>("twice")
                    .unwrap()
                    .call::<_, i64>((21,))
                    .unwrap(),
                42,
            );

            let module = guest.import("host_lib").unwrap();
            let message = module
                .call::<_, i64>(())
                .err()
                .unwrap()
                .to_string();

            assert!(message.contains("callable") || message.contains("not callable"));
        }
    }

    guest_fixture! {
        pub fn import_resolves_a_standard_library_module<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder();
        |guest| {
            assert!(
                guest
                    .import("dataclasses")
                    .unwrap()
                    .function("is_dataclass")
                    .is_ok()
            );
        }
    }

    guest_fixture! {
        pub fn import_resolves_a_standard_library_submodule_to_its_leaf<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder();
        |guest| {
            assert!(
                guest
                    .import("os.path")
                    .unwrap()
                    .function("join")
                    .is_ok()
            );
        }
    }

    guest_fixture! {
        pub fn import_keeps_denied_standard_library_modules_denied<B>()
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
        using Runtime::<B>::builder().deny("subprocess");
        |guest| {
            assert!(
                guest
                    .import("subprocess")
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("denied")
            );
        }
    }

    struct AsyncBox;

    impl HostClass for AsyncBox {
        const NAME: &'static str = "AsyncBox";
    }

    impl<B> HostClassDefinition<B> for AsyncBox
    where
        B: Backend
            + BackendValues
            + BackendCallables
            + BackendClasses
            + BackendModules
            + BackendCoroutines
            + BackendExceptions,
    {
        fn build(builder: &mut ClassBuilder<B, Self>) {
            builder
                .constructor(|_, args| {
                    args.finish()?;

                    Ok(Self)
                })
                .async_method(Dunder::AEnter, |receiver, _, args| {
                    args.finish()?;

                    let this = receiver.resolve::<Object<B>>()?;

                    Ok::<_, Error>(async move { Ok::<_, Error>(this) })
                })
                .async_method(Dunder::AExit, |_, _, _| {
                    Ok::<_, Error>(async { Ok::<_, Error>(false) })
                });
        }
    }

    struct AsyncBoxByName;

    impl HostClass for AsyncBoxByName {
        const NAME: &'static str = "AsyncBoxByName";
    }

    impl<B> HostClassDefinition<B> for AsyncBoxByName
    where
        B: Backend
            + BackendValues
            + BackendCallables
            + BackendClasses
            + BackendModules
            + BackendCoroutines
            + BackendExceptions,
    {
        fn build(builder: &mut ClassBuilder<B, Self>) {
            builder
                .constructor(|_, args| {
                    args.finish()?;

                    Ok(Self)
                })
                .async_method("__aenter__", |receiver, _, args| {
                    args.finish()?;

                    let this = receiver.resolve::<Object<B>>()?;

                    Ok::<_, Error>(async move { Ok::<_, Error>(this) })
                })
                .async_method(Dunder::AExit, |_, _, _| {
                    Ok::<_, Error>(async { Ok::<_, Error>(false) })
                });
        }
    }

    struct Awaitable {
        value: i64,
    }

    impl HostClass for Awaitable {
        const NAME: &'static str = "Awaitable";
    }

    impl<B> HostClassDefinition<B> for Awaitable
    where
        B: Backend
            + BackendValues
            + BackendCallables
            + BackendClasses
            + BackendModules
            + BackendCoroutines
            + BackendExceptions,
    {
        fn build(builder: &mut ClassBuilder<B, Self>) {
            builder
                .constructor(|enter, args| {
                    let value = args.required::<i64>(enter, 0, "value")?;

                    args.finish()?;

                    Ok(Self { value })
                })
                .async_method(Dunder::Await, |receiver, _, args| {
                    args.finish()?;

                    let value = receiver.payload::<Self>()?.value;

                    Ok::<_, Error>(async move { Ok::<_, Error>(value) })
                });
        }
    }

    struct Sequence {
        value: Cell<i64>,
    }

    impl HostClass for Sequence {
        const NAME: &'static str = "Sequence";
    }

    impl<B> HostClassDefinition<B> for Sequence
    where
        B: Backend
            + BackendValues
            + BackendCallables
            + BackendClasses
            + BackendModules
            + BackendCoroutines
            + BackendExceptions,
    {
        fn build(builder: &mut ClassBuilder<B, Self>) {
            builder
                .constructor(|_, args| {
                    args.finish()?;

                    Ok(Self { value: Cell::new(0) })
                })
                .method(Dunder::Aiter, |receiver, _, args| {
                    args.finish()?;

                    receiver.resolve::<Object<B>>()
                })
                .async_method(Dunder::Anext, |receiver, _, args| {
                    args.finish()?;

                    let sequence = receiver.payload::<Self>()?;
                    let next = sequence.value.get() + 1;

                    if next > 2 {
                        return Err(Error::StopAsyncIteration);
                    }

                    sequence.value.set(next);

                    Ok::<_, Error>(async move { Ok::<_, Error>(next) })
                });
        }
    }

    struct Session {
        values: HashMap<String, i64>,
    }

    impl HostClass for Session {
        const NAME: &'static str = "Session";
    }

    impl<B> HostClassDefinition<B> for Session
    where
        B: Backend
            + BackendValues
            + BackendCallables
            + BackendClasses
            + BackendModules
            + BackendCoroutines
            + BackendExceptions,
    {
        fn build(builder: &mut ClassBuilder<B, Self>) {
            builder
                .constructor(|_, args| {
                    args.finish()?;

                    Ok(Self { values: HashMap::new() })
                })
                .method(Dunder::SetItem, |receiver, enter, args| {
                    let key = args.required::<String>(enter, 0, "key")?;
                    let value = args.required::<i64>(enter, 1, "value")?;

                    args.finish()?;

                    receiver
                        .payload_mut::<Self>()?
                        .values
                        .insert(key, value);

                    Ok::<_, Error>(())
                })
                .method(Dunder::GetItem, |receiver, enter, args| {
                    let key = args.required::<String>(enter, 0, "key")?;

                    args.finish()?;

                    receiver
                        .payload::<Self>()?
                        .values
                        .get(&key)
                        .copied()
                        .ok_or_else(|| Error::attribute(key))
                })
                .method(Dunder::Enter, |receiver, _, args| {
                    args.finish()?;

                    receiver.resolve::<Object<B>>()
                })
                .method(Dunder::Exit, |_, _, _| Ok::<_, Error>(false));
        }
    }

    struct RejectsAsyncLen;

    impl HostClass for RejectsAsyncLen {
        const NAME: &'static str = "RejectsAsyncLen";
    }

    impl<B> HostClassDefinition<B> for RejectsAsyncLen
    where
        B: Backend
            + BackendValues
            + BackendCallables
            + BackendClasses
            + BackendModules
            + BackendCoroutines
            + BackendExceptions,
    {
        fn build(builder: &mut ClassBuilder<B, Self>) {
            builder
                .constructor(|_, args| {
                    args.finish()?;

                    Ok(Self)
                })
                .async_method(Dunder::Len, |_, _, _| {
                    Ok::<_, Error>(async { Ok::<_, Error>(0_i64) })
                });
        }
    }

    guest_fixture! {
        pub async fn async_with_over_dunder_registered_aenter_returns_this<B>()
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
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("host_lib").class::<AsyncBox>().expect("AsyncBox registers cleanly"));
        |guest| {
            guest.exec("import host_lib").unwrap();
            guest.exec("async def run():\n    box = host_lib.AsyncBox()\n    async with box as opened:\n        return opened is box\n").unwrap();

            assert!(guest.eval::<Coroutine<B, bool>>("run()").unwrap().await.unwrap());
        }
    }

    guest_fixture! {
        pub async fn async_with_over_string_registered_aenter_returns_this<B>()
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
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("host_lib").class::<AsyncBoxByName>().expect("AsyncBoxByName registers cleanly"));
        |guest| {
            guest.exec("import host_lib").unwrap();
            guest.exec("async def run():\n    box = host_lib.AsyncBoxByName()\n    async with box as opened:\n        return opened is box\n").unwrap();

            assert!(guest.eval::<Coroutine<B, bool>>("run()").unwrap().await.unwrap());
        }
    }

    guest_fixture! {
        pub async fn awaiting_a_host_object_yields_its_pending_value<B>()
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
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("host_lib").class::<Awaitable>().expect("Awaitable registers cleanly"));
        |guest| {
            guest.exec("import host_lib").unwrap();
            guest.exec("async def run():\n    return await host_lib.Awaitable(7)\n").unwrap();

            assert_eq!(guest.eval::<Coroutine<B, i64>>("run()").unwrap().await.unwrap(), 7);
        }
    }

    guest_fixture! {
        pub async fn async_iteration_ends_with_stop_async_iteration<B>()
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
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("host_lib").class::<Sequence>().expect("Sequence registers cleanly"));
        |guest| {
            guest.exec("import host_lib").unwrap();

            assert_eq!(
                guest
                    .eval::<AsyncIter<B, i64>>("host_lib.Sequence()")
                    .unwrap()
                    .collect()
                    .await
                    .unwrap(),
                vec![1, 2],
            );
        }
    }

    guest_fixture! {
        pub fn setitem_mutates_then_getitem_reads_it_back<B>()
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
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("host_lib").class::<Session>().expect("Session registers cleanly"));
        |guest| {
            guest.exec("import host_lib\nsession = host_lib.Session()\nsession[\"count\"] = 3\n").unwrap();

            assert!(guest.eval::<bool>(r#"session["count"] == 3"#).unwrap());
        }
    }

    guest_fixture! {
        pub fn enter_returns_the_same_object<B>()
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
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("host_lib").class::<Session>().expect("Session registers cleanly"));
        |guest| {
            guest.exec("import host_lib\nsession = host_lib.Session()\nwith session as opened:\n    result = opened is session\n").unwrap();

            assert!(guest.eval::<bool>("result").unwrap());
        }
    }

    guest_fixture! {
        pub fn imported_mapping_uses_python_protocols<B>()
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
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("host_lib").class::<HostMapping>().unwrap());
        |guest| {
            guest
                .exec(
                    r#"
import collections.abc
import host_lib

mapping = host_lib.HostMapping()
assert isinstance(mapping, collections.abc.Mapping)
assert type(type(mapping)) is collections.abc.ABCMeta
assert dict(mapping) == {'answer': 42}
assert list(mapping.items()) == [('answer', 42)]
assert mapping.get('missing', 1) == 1
assert mapping == {'answer': 42}
assert host_lib.HostMapping[str]
"#,
                )
                .unwrap();
        }
    }

    guest_fixture! {
        pub fn mixed_bases_preserve_mro_order<B>()
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
        using Runtime::<B>::builder()
            .bind(
                ModuleSpec::new("host_lib")
                    .class::<MappingParent>()
                    .unwrap()
                    .class::<MixedMapping>()
                    .unwrap(),
            );
        |guest| {
            guest.exec("import host_lib").unwrap();

            let names = guest
                .eval::<Class<B>>("host_lib.MixedMapping")
                .unwrap()
                .mro()
                .unwrap()
                .iter()
                .map(Named::name)
                .collect::<Result<Vec<_>, _>>()
                .unwrap();

            assert_eq!(names[0], "MixedMapping");
            assert_eq!(names[1], "MappingParent");
            assert_eq!(names[3], "Mapping");
        }
    }

    guest_fixture! {
        pub fn abstract_imported_base_is_completed_by_a_host_subclass<B>()
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
        using Runtime::<B>::builder()
            .bind(
                ModuleSpec::new("host_lib")
                    .class::<AbstractMapping>()
                    .unwrap()
                    .class::<ConcreteMapping>()
                    .unwrap(),
            );
        |guest| {
            guest.exec("import host_lib").unwrap();

            let abstract_class = guest
                .eval::<Class<B>>("host_lib.AbstractMapping")
                .unwrap();
            let concrete_class = guest
                .eval::<Class<B>>("host_lib.ConcreteMapping")
                .unwrap();

            assert_eq!(
                abstract_class.abstract_methods().unwrap(),
                vec![String::from("__len__")],
            );
            assert!(concrete_class.abstract_methods().unwrap().is_empty());
            assert!(
                guest
                    .eval::<Class<B>>("object")
                    .unwrap()
                    .abstract_methods()
                    .unwrap()
                    .is_empty(),
            );

            let guest_error = guest
                .eval::<Instance<B>>("host_lib.AbstractMapping()")
                .err()
                .unwrap();

            assert!(guest_error.to_string().contains("TypeError"));
            assert!(guest_error.to_string().contains("__len__"));

            let host_error = guest
                .enter(|enter| AbstractMapping.to_guest(enter).map(|_| ()))
                .err()
                .unwrap();

            assert!(matches!(host_error, Error::Raise(_)));
            assert!(host_error.to_string().contains("TypeError"));
            assert!(host_error.to_string().contains("__len__"));

            guest
                .exec(
                    r#"
value = host_lib.ConcreteMapping()
assert isinstance(value, host_lib.AbstractMapping)
assert len(value) == 1
"#,
                )
                .unwrap();
        }
    }

    pub fn two_guests_share_one_realised_imported_base_class<B>()
    where
        B: Backend
            + BackendValues
            + BackendCallables
            + BackendClasses
            + BackendModules
            + BackendCoroutines
            + BackendExceptions
            + BackendInterrupt,
    {
        let runtime = Runtime::<B>::builder()
            .bind(
                ModuleSpec::new("host_lib")
                    .class::<HostMapping>()
                    .unwrap()
                    .function("mapping_class", |enter, args| {
                        args.finish()?;
                        Class::<B>::of::<HostMapping>(enter)
                    }),
            )
            .build()
            .unwrap();
        let first = runtime.guest().build().unwrap();
        let second = runtime.guest().build().unwrap();

        first.exec("import host_lib").unwrap();
        second.exec("import host_lib").unwrap();

        let first_class = first
            .eval::<Class<B>>("host_lib.mapping_class()")
            .unwrap();
        let second_class = second
            .eval::<Class<B>>("host_lib.mapping_class()")
            .unwrap();

        assert!(
            first_class
                .value()
                .ptr_eq(&second_class.value())
        );
    }

    pub fn async_len_is_rejected_at_build_time<B>()
    where
        B: Backend
            + BackendValues
            + BackendCallables
            + BackendClasses
            + BackendModules
            + BackendCoroutines
            + BackendExceptions,
    {
        assert!(matches!(
            ModuleSpec::<B>::new("host_lib").class::<RejectsAsyncLen>(),
            Err(Error::Unsupported { .. })
        ));
    }

    #[doc(hidden)]
    #[macro_export]
    macro_rules! __guestpy_backend_classes_tests {
        ($backend:ty) => {
            #[test]
            fn host_borrows_dynamic_and_typed_payloads() {
                $crate::backend::classes::fixtures::host_borrows_dynamic_and_typed_payloads::<
                    $backend,
                >();
            }

            #[test]
            fn ordinary_guest_class_uses_instance_handle() {
                $crate::backend::classes::fixtures::ordinary_guest_class_uses_instance_handle::<
                    $backend,
                >();
            }

            #[test]
            fn class_handle_rejects_other_callables() {
                $crate::backend::classes::fixtures::class_handle_rejects_other_callables::<
                    $backend,
                >();
            }

            #[test]
            fn class_constructs_and_overrides_its_result_descriptor() {
                $crate::backend::classes::fixtures::class_constructs_and_overrides_its_result_descriptor::<
                    $backend,
                >();
            }

            #[test]
            fn guest_subclass_works() {
                $crate::backend::classes::fixtures::guest_subclass_works::<$backend>();
            }

            #[test]
            fn subclass_that_skips_super_init_fails_clearly() {
                $crate::backend::classes::fixtures::subclass_that_skips_super_init_fails_clearly::<
                    $backend,
                >();
            }

            #[test]
            fn class_handle_reads_class_attributes() {
                $crate::backend::classes::fixtures::class_handle_reads_class_attributes::<
                    $backend,
                >();
            }

            #[test]
            fn module_and_function_expose_names_and_annotations() {
                $crate::backend::classes::fixtures::module_and_function_expose_names_and_annotations::<
                    $backend,
                >();
            }

            #[test]
            fn class_reports_bases_and_mro() {
                $crate::backend::classes::fixtures::class_reports_bases_and_mro::<$backend>();
            }

            #[test]
            fn isinstance_and_issubclass_agree_across_backends() {
                $crate::backend::classes::fixtures::isinstance_and_issubclass_agree_across_backends::<
                    $backend,
                >();
            }

            #[test]
            fn host_class_is_generic() {
                $crate::backend::classes::fixtures::host_class_is_generic::<$backend>();
            }

            #[tokio::test]
            async fn host_base_method_calls_guest_override() {
                $crate::backend::classes::fixtures::host_base_method_calls_guest_override::<
                    $backend,
                >()
                .await;
            }

            #[test]
            fn paired_method_reads_payload_and_calls_override() {
                $crate::backend::classes::fixtures::paired_method_reads_payload_and_calls_override::<
                    $backend,
                >();
            }

            #[tokio::test]
            async fn paired_async_method_reads_payload_and_calls_override() {
                $crate::backend::classes::fixtures::paired_async_method_reads_payload_and_calls_override::<
                    $backend,
                >()
                .await;
            }

            #[test]
            fn annotations_preserve_declaration_order() {
                $crate::backend::classes::fixtures::annotations_preserve_declaration_order::<
                    $backend,
                >();
            }

            #[test]
            fn any_handle_calls_a_callable_and_reports_a_clear_error_otherwise() {
                $crate::backend::classes::fixtures::any_handle_calls_a_callable_and_reports_a_clear_error_otherwise::<
                    $backend,
                >();
            }

            #[test]
            fn import_resolves_a_standard_library_module() {
                $crate::backend::classes::fixtures::import_resolves_a_standard_library_module::<
                    $backend,
                >();
            }

            #[test]
            fn import_resolves_a_standard_library_submodule_to_its_leaf() {
                $crate::backend::classes::fixtures::import_resolves_a_standard_library_submodule_to_its_leaf::<
                    $backend,
                >();
            }

            #[test]
            fn import_keeps_denied_standard_library_modules_denied() {
                $crate::backend::classes::fixtures::import_keeps_denied_standard_library_modules_denied::<
                    $backend,
                >();
            }

            #[tokio::test]
            async fn async_with_over_dunder_registered_aenter_returns_this() {
                $crate::backend::classes::fixtures::async_with_over_dunder_registered_aenter_returns_this::<
                    $backend,
                >()
                .await;
            }

            #[tokio::test]
            async fn async_with_over_string_registered_aenter_returns_this() {
                $crate::backend::classes::fixtures::async_with_over_string_registered_aenter_returns_this::<
                    $backend,
                >()
                .await;
            }

            #[tokio::test]
            async fn awaiting_a_host_object_yields_its_pending_value() {
                $crate::backend::classes::fixtures::awaiting_a_host_object_yields_its_pending_value::<
                    $backend,
                >()
                .await;
            }

            #[tokio::test]
            async fn async_iteration_ends_with_stop_async_iteration() {
                $crate::backend::classes::fixtures::async_iteration_ends_with_stop_async_iteration::<
                    $backend,
                >()
                .await;
            }

            #[test]
            fn setitem_mutates_then_getitem_reads_it_back() {
                $crate::backend::classes::fixtures::setitem_mutates_then_getitem_reads_it_back::<
                    $backend,
                >();
            }

            #[test]
            fn enter_returns_the_same_object() {
                $crate::backend::classes::fixtures::enter_returns_the_same_object::<$backend>();
            }

            #[test]
            fn async_len_is_rejected_at_build_time() {
                $crate::backend::classes::fixtures::async_len_is_rejected_at_build_time::<
                    $backend,
                >();
            }

            #[test]
            fn imported_mapping_uses_python_protocols() {
                $crate::backend::classes::fixtures::imported_mapping_uses_python_protocols::<
                    $backend,
                >();
            }

            #[test]
            fn mixed_bases_preserve_mro_order() {
                $crate::backend::classes::fixtures::mixed_bases_preserve_mro_order::<
                    $backend,
                >();
            }

            #[test]
            fn abstract_imported_base_is_completed_by_a_host_subclass() {
                $crate::backend::classes::fixtures::abstract_imported_base_is_completed_by_a_host_subclass::<
                    $backend,
                >();
            }

            #[test]
            fn two_guests_share_one_realised_imported_base_class() {
                $crate::backend::classes::fixtures::two_guests_share_one_realised_imported_base_class::<
                    $backend,
                >();
            }
        };
    }

    pub use crate::__guestpy_backend_classes_tests as tests;
}
