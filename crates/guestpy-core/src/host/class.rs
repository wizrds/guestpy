use std::{
    any::TypeId, borrow::Cow, cell::RefCell, collections::HashMap, future::Future,
    marker::PhantomData, rc::Rc, str::FromStr,
};

use crate::{
    backend::{
        Backend, BackendCallables, BackendClasses, BackendCoroutines, BackendModules,
        BackendValues, Val,
        callables::{HostBody, PendingValue, RawBody},
    },
    errors::Error,
    handle::{Class, TypeProtocol, Value},
    host::{
        context::{FromContext, Requirements},
        declaration::{DeclarationContext, DeclareMember, Member},
        dunder::Dunder,
        exception::{ExceptionClass, Raise},
        namespace::{Namespace, ValueDeclaration},
        receiver::Receiver,
    },
    imports::Imports,
    marshal::{FromGuest, ToGuest, args::Args},
    scope::Enter,
};

pub(crate) type AllocBody<B> =
    Rc<dyn for<'py> Fn(&Enter<'py, B>, Val<'py, B>) -> Result<Val<'py, B>, Error>>;

pub(crate) type InitBody<B> =
    Rc<dyn for<'py> Fn(&Enter<'py, B>, Val<'py, B>, Args<'py, B>) -> Result<(), Error>>;

pub(crate) type MethodBody<B> =
    Rc<dyn for<'py> Fn(&Enter<'py, B>, Val<'py, B>, Args<'py, B>) -> Result<Val<'py, B>, Error>>;

pub(crate) type SetterBody<B> =
    Rc<dyn for<'py> Fn(&Enter<'py, B>, Val<'py, B>, Val<'py, B>) -> Result<(), Error>>;

pub(crate) type DeleterBody<B> =
    Rc<dyn for<'py> Fn(&Enter<'py, B>, Val<'py, B>) -> Result<(), Error>>;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum MemberName {
    Named(String),
    Dunder(Dunder),
}

impl MemberName {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Named(name) => name,
            Self::Dunder(dunder) => dunder.name(),
        }
    }
}

impl From<Dunder> for MemberName {
    fn from(value: Dunder) -> Self {
        Self::Dunder(value)
    }
}

impl From<&str> for MemberName {
    fn from(value: &str) -> Self {
        match Dunder::from_str(value) {
            Ok(dunder) => Self::Dunder(dunder),
            Err(_) => Self::Named(value.to_owned()),
        }
    }
}

impl From<String> for MemberName {
    fn from(value: String) -> Self {
        match Dunder::from_str(&value) {
            Ok(dunder) => Self::Dunder(dunder),
            Err(_) => Self::Named(value),
        }
    }
}

struct MethodDeclaration<B: Backend> {
    body: MethodBody<B>,
}

impl<B: Backend> MethodDeclaration<B> {
    fn new<F>(body: F) -> Self
    where
        F: for<'py> Fn(&Enter<'py, B>, Val<'py, B>, Args<'py, B>) -> Result<Val<'py, B>, Error>
            + 'static,
    {
        Self { body: Rc::new(body) }
    }
}

impl<B> DeclareMember<B> for MethodDeclaration<B>
where
    B: Backend + BackendValues + BackendCallables,
{
    fn realise<'py>(
        &self,
        context: &DeclarationContext<'py, '_, B>,
        name: &str,
    ) -> Result<Val<'py, B>, Error> {
        B::method(context.enter().token(), name, None, context.method_raw_body(self.body.clone()))
    }
}

struct ClassMethodDeclaration<B: Backend> {
    body: MethodBody<B>,
}

impl<B: Backend> ClassMethodDeclaration<B> {
    fn new<F>(body: F) -> Self
    where
        F: for<'py> Fn(&Enter<'py, B>, Val<'py, B>, Args<'py, B>) -> Result<Val<'py, B>, Error>
            + 'static,
    {
        Self { body: Rc::new(body) }
    }
}

impl<B> DeclareMember<B> for ClassMethodDeclaration<B>
where
    B: Backend + BackendValues + BackendCallables,
{
    fn realise<'py>(
        &self,
        context: &DeclarationContext<'py, '_, B>,
        name: &str,
    ) -> Result<Val<'py, B>, Error> {
        let function = B::function(
            context.enter().token(),
            name,
            None,
            context.method_raw_body(self.body.clone()),
        )?;

        context.wrap_builtin("classmethod", function)
    }
}

struct StaticMethodDeclaration<B: Backend> {
    body: HostBody<B>,
}

impl<B: Backend> StaticMethodDeclaration<B> {
    fn new<F>(body: F) -> Self
    where
        F: for<'py> Fn(&Enter<'py, B>, Args<'py, B>) -> Result<Val<'py, B>, Error> + 'static,
    {
        Self { body: Rc::new(body) }
    }
}

impl<B> DeclareMember<B> for StaticMethodDeclaration<B>
where
    B: Backend + BackendValues + BackendCallables,
{
    fn realise<'py>(
        &self,
        context: &DeclarationContext<'py, '_, B>,
        name: &str,
    ) -> Result<Val<'py, B>, Error> {
        let function = B::function(
            context.enter().token(),
            name,
            None,
            context
                .enter()
                .guest()
                .raw_body(self.body.clone()),
        )?;

        context.wrap_builtin("staticmethod", function)
    }
}

struct ClassPropertyDeclaration<B: Backend> {
    get: RefCell<Option<MethodBody<B>>>,
    set: RefCell<Option<SetterBody<B>>>,
    del: RefCell<Option<DeleterBody<B>>>,
}

impl<B: Backend> ClassPropertyDeclaration<B> {
    fn new() -> Self {
        Self {
            get: RefCell::new(None),
            set: RefCell::new(None),
            del: RefCell::new(None),
        }
    }

    fn set_get<F>(&self, get: F)
    where
        F: for<'py> Fn(&Enter<'py, B>, Val<'py, B>, Args<'py, B>) -> Result<Val<'py, B>, Error>
            + 'static,
    {
        *self.get.borrow_mut() = Some(Rc::new(get));
    }

    fn set_set<F>(&self, set: F)
    where
        F: for<'py> Fn(&Enter<'py, B>, Val<'py, B>, Val<'py, B>) -> Result<(), Error> + 'static,
    {
        *self.set.borrow_mut() = Some(Rc::new(set));
    }

    fn set_del<F>(&self, del: F)
    where
        F: for<'py> Fn(&Enter<'py, B>, Val<'py, B>) -> Result<(), Error> + 'static,
    {
        *self.del.borrow_mut() = Some(Rc::new(del));
    }
}

impl<B> DeclareMember<B> for ClassPropertyDeclaration<B>
where
    B: Backend + BackendValues + BackendCallables,
{
    fn realise<'py>(
        &self,
        context: &DeclarationContext<'py, '_, B>,
        name: &str,
    ) -> Result<Val<'py, B>, Error> {
        let getter = self
            .get
            .borrow()
            .clone()
            .map(|get| {
                B::function(context.enter().token(), name, None, context.method_raw_body(get))
            })
            .transpose()?;
        let setter = self
            .set
            .borrow()
            .clone()
            .map(|set| {
                B::function(
                    context.enter().token(),
                    name,
                    None,
                    context
                        .enter()
                        .guest()
                        .raw_body(Rc::new(move |enter, args| {
                            let (receiver, args) = args.split_receiver()?;

                            set(
                                enter,
                                receiver,
                                args.required::<Value<B>>(enter, 0, "value")?
                                    .to_guest(enter)?,
                            )?;

                            Ok(B::none(enter.token()))
                        })),
                )
            })
            .transpose()?;
        let deleter = self
            .del
            .borrow()
            .clone()
            .map(|del| {
                B::function(
                    context.enter().token(),
                    name,
                    None,
                    context
                        .enter()
                        .guest()
                        .raw_body(Rc::new(move |enter, args| {
                            del(enter, args.split_receiver()?.0)?;

                            Ok(B::none(enter.token()))
                        })),
                )
            })
            .transpose()?;

        context.property(getter, setter, deleter)
    }
}

pub(crate) enum ClassBase<B: Backend> {
    Host(Rc<ClassSpec<B>>),
    Imported {
        module: Cow<'static, str>,
        qualname: Cow<'static, str>,
    },
}

pub struct ClassSpec<B: Backend> {
    name: &'static str,
    doc: Option<&'static str>,
    module: RefCell<Option<String>>,
    bases: Vec<ClassBase<B>>,
    alloc: AllocBody<B>,
    init: InitBody<B>,
    members: Vec<(MemberName, Member<B>)>,
    statics: Namespace<B>,
    payload: TypeId,
    requirements: Requirements<B>,
}

impl<B: Backend> ClassSpec<B> {
    fn push_member(&mut self, name: MemberName, member: Member<B>) {
        self.members.push((name, member));
    }

    fn push_base(&mut self, base: ClassBase<B>) {
        self.bases.push(base);
    }

    pub(crate) fn abstract_error(name: &str, names: &[String]) -> Error {
        Raise::<B>::new(ExceptionClass::builtin("TypeError"))
            .text(format!(
                "Can't instantiate abstract class {name} with abstract methods {}",
                names.join(", "),
            ))
            .into()
    }

    pub(crate) fn payload(&self) -> TypeId {
        self.payload
    }

    pub(crate) fn requirements(&self) -> &Requirements<B> {
        &self.requirements
    }

    pub(crate) fn host_lineage(self: &Rc<Self>) -> Vec<Rc<ClassSpec<B>>> {
        let mut lineage = vec![self.clone()];

        for base in &self.bases {
            if let ClassBase::Host(base) = base {
                lineage.extend(base.host_lineage());
            }
        }

        lineage
    }

    pub(crate) fn doc(&self) -> Option<&'static str> {
        self.doc
    }

    pub(crate) fn module_name(&self) -> Option<String> {
        self.module.borrow().clone()
    }

    pub(crate) fn name(&self) -> &'static str {
        self.name
    }

    pub(crate) fn bases(&self) -> &[ClassBase<B>] {
        &self.bases
    }

    pub(crate) fn alloc(&self) -> &AllocBody<B> {
        &self.alloc
    }

    pub(crate) fn init(&self) -> &InitBody<B> {
        &self.init
    }

    pub(crate) fn members(&self) -> &[(MemberName, Member<B>)] {
        &self.members
    }

    pub(crate) fn statics(&self) -> &Namespace<B> {
        &self.statics
    }

    pub(crate) fn set_module(&self, module: &str) {
        self.module
            .borrow_mut()
            .get_or_insert_with(|| module.to_owned());
    }
}

impl<B> ClassSpec<B>
where
    B: Backend + BackendValues + BackendCallables + BackendClasses,
{
    pub(crate) fn of<C>() -> Result<Rc<ClassSpec<B>>, Error>
    where
        C: HostClass + HostClassDefinition<B>,
    {
        thread_local! {
            static BUILDING: RefCell<Vec<TypeId>> = const { RefCell::new(Vec::new()) };
        }

        let type_id = TypeId::of::<C>();

        BUILDING.with(|building| {
            assert!(
                !building.borrow().contains(&type_id),
                "host class cycle: {} is its own base",
                C::NAME,
            );

            building.borrow_mut().push(type_id);
        });

        let mut builder = ClassBuilder::<B, C>::new();

        C::build(&mut builder);

        BUILDING.with(|building| {
            building.borrow_mut().pop();
        });

        Ok(Rc::new(builder.finish()?))
    }

    pub(crate) fn realise_registered<'py, C>(enter: &Enter<'py, B>) -> Result<Val<'py, B>, Error>
    where
        B: BackendModules,
        C: HostClass + HostClassDefinition<B>,
    {
        let spec = enter
            .guest()
            .realisation()
            .class_spec(TypeId::of::<C>())
            .ok_or_else(|| {
                Error::unexpected(format!("host class {} was not registered", C::NAME))
            })?;

        ClassRealiser::new(enter).realise(&spec)
    }
}

struct ClassRealiser<'py, 'e, B: Backend> {
    enter: &'e Enter<'py, B>,
}

impl<'py, 'e, B> ClassRealiser<'py, 'e, B>
where
    B: Backend + BackendValues + BackendCallables + BackendClasses + BackendModules,
{
    fn new(enter: &'e Enter<'py, B>) -> Self {
        Self { enter }
    }

    fn class_new(&self, spec: &Rc<ClassSpec<B>>) -> Result<Val<'py, B>, Error> {
        let alloc = spec.alloc().clone();
        let name = spec.name();

        B::function(
            self.enter.token(),
            "__new__",
            None,
            self.enter
                .guest()
                .raw_body(Rc::new(move |enter, args| {
                    let class = args.split_receiver()?.0;
                    let names = Class::<B>::from_guest(enter, class.clone())?.abstract_methods()?;

                    if !names.is_empty() {
                        return Err(ClassSpec::<B>::abstract_error(name, &names));
                    }

                    alloc(enter, class)
                })),
        )
    }

    fn class_init(&self, spec: &Rc<ClassSpec<B>>) -> Result<RawBody<B>, Error> {
        let init = spec.init().clone();

        Ok(self
            .enter
            .guest()
            .raw_body(Rc::new(move |enter, args| {
                let (instance, args) = args.split_receiver()?;

                init(enter, instance, args)?;

                Ok(B::none(enter.token()))
            })))
    }

    fn bases(&self, spec: &Rc<ClassSpec<B>>) -> Result<Vec<Val<'py, B>>, Error> {
        let imports = Imports::new(self.enter);
        let mut bases = Vec::new();
        let mut has_host_base = false;

        for base in spec.bases() {
            match base {
                ClassBase::Host(base) => {
                    has_host_base = true;
                    bases.push(self.realise(base)?);
                }
                ClassBase::Imported { module, qualname } => {
                    let base = imports.qualified(&imports.external(module)?, qualname)?;

                    if !B::is_class(self.enter.token(), &base) {
                        return Err(Error::conversion(format!(
                            "{module}.{qualname} is not a class",
                        )));
                    }

                    bases.push(base);
                }
            }
        }

        if !has_host_base {
            bases.insert(0, B::native_base(self.enter.token()));
        }

        Ok(bases)
    }

    fn metaclass(&self, bases: &[Val<'py, B>]) -> Result<Val<'py, B>, Error> {
        let mut winner = B::get_item(
            self.enter.token(),
            &B::builtins_dict(self.enter.token())?,
            &B::str(self.enter.token(), "type"),
        )?;

        for base in bases {
            let candidate = B::get_attr(self.enter.token(), base, "__class__")?;

            if B::is_subclass(self.enter.token(), &candidate, &winner)? {
                winner = candidate;
            }
        }

        Ok(winner)
    }

    fn realise(&self, spec: &Rc<ClassSpec<B>>) -> Result<Val<'py, B>, Error> {
        let realisation = self.enter.guest().realisation();
        let payload = spec.payload();

        if !realisation.class_registered(payload) {
            return Err(Error::unexpected("host class was not registered"));
        }

        if let Some(owned) = realisation.realised_class(payload) {
            return Ok(B::attach(self.enter.token(), &owned));
        }

        let bases = self.bases(spec)?;
        let metaclass = self.metaclass(&bases)?;
        let bases = B::tuple(self.enter.token(), bases)?;
        let context = DeclarationContext::new(self.enter);
        let namespace = B::call(
            self.enter.token(),
            &B::get_attr(self.enter.token(), &metaclass, "__prepare__")?,
            &[B::str(self.enter.token(), spec.name()), bases.clone()],
            &[],
        )?;

        for (member_name, member) in spec.members() {
            let attribute = member_name.as_str();
            let value = member.realise(&context, attribute)?;

            B::set_item(
                self.enter.token(),
                &namespace,
                B::str(self.enter.token(), attribute),
                value,
            )?;
        }

        B::set_item(
            self.enter.token(),
            &namespace,
            B::str(self.enter.token(), "__doc__"),
            spec.doc()
                .map_or_else(|| B::none(self.enter.token()), |doc| B::str(self.enter.token(), doc)),
        )?;
        B::set_item(
            self.enter.token(),
            &namespace,
            B::str(self.enter.token(), "__module__"),
            B::str(
                self.enter.token(),
                spec.module_name()
                    .as_deref()
                    .unwrap_or(""),
            ),
        )?;
        B::set_item(
            self.enter.token(),
            &namespace,
            B::str(self.enter.token(), "__new__"),
            context.wrap_builtin("staticmethod", self.class_new(spec)?)?,
        )?;
        B::set_item(
            self.enter.token(),
            &namespace,
            B::str(self.enter.token(), "__init__"),
            B::method(self.enter.token(), "__init__", None, self.class_init(spec)?)?,
        )?;

        let class = B::call(
            self.enter.token(),
            &metaclass,
            &[B::str(self.enter.token(), spec.name()), bases, namespace],
            &[],
        )?;

        for (name, member) in spec.statics().members() {
            B::set_attr(self.enter.token(), &class, name, member.realise(&context, name)?)?;
        }

        realisation.set_realised_class(payload, B::detach(self.enter.token(), class.clone()));

        Ok(class)
    }
}

pub(crate) struct ClassDeclaration<B: Backend> {
    spec: Rc<ClassSpec<B>>,
}

impl<B: Backend> ClassDeclaration<B> {
    pub(crate) fn new(spec: Rc<ClassSpec<B>>) -> Self {
        Self { spec }
    }
}

impl<B> DeclareMember<B> for ClassDeclaration<B>
where
    B: Backend + BackendValues + BackendCallables + BackendClasses + BackendModules,
{
    fn realise<'py>(
        &self,
        context: &DeclarationContext<'py, '_, B>,
        _name: &str,
    ) -> Result<Val<'py, B>, Error> {
        ClassRealiser::new(context.enter()).realise(&self.spec)
    }
}

pub trait HostClass: Sized + 'static {
    const NAME: &'static str;
    const DOC: Option<&'static str> = None;
}

pub trait HostClassDefinition<B: Backend>: HostClass {
    fn build(builder: &mut ClassBuilder<B, Self>);
}

pub struct ClassBuilder<B: Backend, C> {
    spec: ClassSpec<B>,
    error: Option<Error>,
    properties: HashMap<String, Rc<ClassPropertyDeclaration<B>>>,
    marker: PhantomData<fn() -> C>,
}

impl<B, C> ClassBuilder<B, C>
where
    B: Backend + BackendValues + BackendCallables + BackendClasses,
    C: HostClass + HostClassDefinition<B>,
{
    fn new() -> Self {
        Self {
            spec: ClassSpec {
                name: C::NAME,
                doc: C::DOC,
                module: RefCell::new(None),
                bases: Vec::new(),
                alloc: Rc::new(|enter, class| B::alloc::<C>(enter.token(), &class)),
                init: Rc::new(|_, _, _| {
                    Err(Error::unsupported(
                        format!("host class {} cannot be constructed", C::NAME,),
                    ))
                }),
                members: Vec::new(),
                statics: Namespace::new(),
                payload: TypeId::of::<C>(),
                requirements: Requirements::new(),
            },
            error: None,
            properties: HashMap::new(),
            marker: PhantomData,
        }
    }

    fn finish(self) -> Result<ClassSpec<B>, Error> {
        match self.error {
            Some(error) => Err(error),
            None => Ok(self.spec),
        }
    }

    fn reject(&mut self, error: Error) {
        if self.error.is_none() {
            self.error = Some(error);
        }
    }

    fn push(&mut self, name: MemberName, member: Member<B>) -> &mut Self {
        self.spec.push_member(name, member);

        self
    }

    fn push_async(&mut self, name: MemberName, member: Member<B>) -> &mut Self {
        if let MemberName::Dunder(dunder) = name
            && !dunder.accepts_awaitable()
        {
            self.reject(Error::unsupported(format!(
                "host class {} cannot declare {dunder} as async",
                C::NAME,
            )));
        }

        self.push(name, member)
    }

    fn property_slot(&mut self, name: &str) -> Rc<ClassPropertyDeclaration<B>> {
        if let Some(property) = self.properties.get(name) {
            return property.clone();
        }

        let property = Rc::new(ClassPropertyDeclaration::new());

        self.properties
            .insert(name.to_owned(), property.clone());
        self.push(name.into(), property.clone());

        property
    }

    pub fn constructor<F>(&mut self, construct: F) -> &mut Self
    where
        F: for<'py> Fn(&Enter<'py, B>, Args<'py, B>) -> Result<C, Error> + 'static,
    {
        self.spec.init = Rc::new(move |enter, instance, args| {
            B::set_payload::<C>(enter.token(), &instance, construct(enter, args)?)
        });

        self
    }

    pub fn require<T: FromContext<B>>(&mut self) -> &mut Self {
        T::declare(&mut self.spec.requirements);

        self
    }

    pub fn method<F, R>(&mut self, name: impl Into<MemberName>, function: F) -> &mut Self
    where
        F: for<'a, 'py> Fn(Receiver<'a, 'py, B>, &Enter<'py, B>, Args<'py, B>) -> Result<R, Error>
            + 'static,
        R: ToGuest<B> + 'static,
    {
        self.push(
            name.into(),
            Rc::new(MethodDeclaration::new(move |enter, receiver, args| {
                function(Receiver::new(enter, &receiver), enter, args)?.to_guest(enter)
            })),
        )
    }

    pub fn class_method<F, R>(&mut self, name: &str, function: F) -> &mut Self
    where
        F: for<'a, 'py> Fn(Receiver<'a, 'py, B>, &Enter<'py, B>, Args<'py, B>) -> Result<R, Error>
            + 'static,
        R: ToGuest<B> + 'static,
    {
        self.push(
            name.into(),
            Rc::new(ClassMethodDeclaration::new(move |enter, class, args| {
                function(Receiver::new(enter, &class), enter, args)?.to_guest(enter)
            })),
        )
    }

    pub fn static_method<F, R>(&mut self, name: &str, function: F) -> &mut Self
    where
        F: for<'py> Fn(&Enter<'py, B>, Args<'py, B>) -> Result<R, Error> + 'static,
        R: ToGuest<B> + 'static,
    {
        self.push(
            name.into(),
            Rc::new(StaticMethodDeclaration::new(move |enter, args| {
                function(enter, args)?.to_guest(enter)
            })),
        )
    }

    pub fn getter<F, R>(&mut self, name: &str, get: F) -> &mut Self
    where
        F: for<'a, 'py> Fn(Receiver<'a, 'py, B>, &Enter<'py, B>) -> Result<R, Error> + 'static,
        R: ToGuest<B> + 'static,
    {
        self.property_slot(name)
            .set_get(move |enter, receiver, _| {
                get(Receiver::new(enter, &receiver), enter)?.to_guest(enter)
            });

        self
    }

    pub fn setter<F, V>(&mut self, name: &str, set: F) -> &mut Self
    where
        F: for<'a, 'py> Fn(Receiver<'a, 'py, B>, &Enter<'py, B>, V) -> Result<(), Error> + 'static,
        V: FromGuest<B, Owned = V> + 'static,
    {
        self.property_slot(name)
            .set_set(move |enter, receiver, value| {
                set(Receiver::new(enter, &receiver), enter, V::from_guest(enter, value)?)
            });

        self
    }

    pub fn deleter<F>(&mut self, name: &str, del: F) -> &mut Self
    where
        F: for<'a, 'py> Fn(Receiver<'a, 'py, B>, &Enter<'py, B>) -> Result<(), Error> + 'static,
    {
        self.property_slot(name)
            .set_del(move |enter, receiver| del(Receiver::new(enter, &receiver), enter));

        self
    }

    pub fn property<G, S, R, V>(&mut self, name: &str, get: G, set: S) -> &mut Self
    where
        G: for<'a, 'py> Fn(Receiver<'a, 'py, B>, &Enter<'py, B>) -> Result<R, Error> + 'static,
        S: for<'a, 'py> Fn(Receiver<'a, 'py, B>, &Enter<'py, B>, V) -> Result<(), Error> + 'static,
        R: ToGuest<B> + 'static,
        V: FromGuest<B, Owned = V> + 'static,
    {
        self.getter(name, get).setter(name, set)
    }

    pub fn constant<V>(&mut self, name: &str, value: V) -> &mut Self
    where
        V: ToGuest<B> + Clone + 'static,
    {
        self.push(name.into(), Rc::new(ValueDeclaration::new(Namespace::constant_thunk(value))))
    }

    pub fn statics<F: FnOnce(&mut Namespace<B>)>(&mut self, build: F) -> &mut Self {
        build(&mut self.spec.statics);

        self
    }

    pub fn generic(&mut self) -> &mut Self {
        self.class_method("__class_getitem__", |receiver, enter, args| {
            let item = args
                .required::<Value<B>>(enter, 0, "item")?
                .to_guest(enter)?;
            let mut arguments = Vec::new();

            if B::is_tuple(enter.token(), &item) {
                let iterator = B::iter(enter.token(), &item)?;

                while let Some(argument) = B::next(enter.token(), &iterator)? {
                    arguments.push(argument);
                }
            } else {
                arguments.push(item);
            }

            Value::<B>::from_guest(
                enter,
                B::generic_alias(enter.token(), receiver.value(), &arguments)?,
            )
        })
    }
}

impl<B, C> ClassBuilder<B, C>
where
    B: Backend
        + BackendValues
        + BackendCallables
        + BackendClasses
        + BackendModules
        + BackendCoroutines,
    C: HostClass + HostClassDefinition<B>,
{
    fn pending<'py, Fut, R>(enter: &Enter<'py, B>, future: Fut) -> Result<B::Value<'py>, Error>
    where
        Fut: Future<Output = Result<R, Error>> + 'static,
        R: ToGuest<B> + 'static,
    {
        enter
            .guest()
            .ensure_async_driver(enter)?
            .driver()
            .register_host_future(enter, PendingValue::<B, R>::into_host_future(future))
    }

    fn awaitable<'py>(
        enter: &Enter<'py, B>,
        name: &MemberName,
        pending: Val<'py, B>,
    ) -> Result<Val<'py, B>, Error> {
        if name == &MemberName::Dunder(Dunder::Await) {
            return B::call(
                enter.token(),
                &B::get_attr(enter.token(), &pending, Dunder::Await.name())?,
                &[],
                &[],
            );
        }

        Ok(pending)
    }

    pub fn async_method<F, Fut, R>(&mut self, name: impl Into<MemberName>, function: F) -> &mut Self
    where
        F: for<'a, 'py> Fn(
                Receiver<'a, 'py, B>,
                &Enter<'py, B>,
                Args<'py, B>,
            ) -> Result<Fut, Error>
            + 'static,
        Fut: Future<Output = Result<R, Error>> + 'static,
        R: ToGuest<B> + 'static,
    {
        let name = name.into();

        self.push_async(
            name.clone(),
            Rc::new(MethodDeclaration::new(move |enter, receiver, args| {
                Self::awaitable(
                    enter,
                    &name,
                    Self::pending(enter, function(Receiver::new(enter, &receiver), enter, args)?)?,
                )
            })),
        )
    }
}

impl<B, C> ClassBuilder<B, C>
where
    B: Backend + BackendValues + BackendCallables + BackendClasses,
    C: HostClass + HostClassDefinition<B>,
{
    pub fn base<P>(&mut self) -> &mut Self
    where
        P: HostClass + HostClassDefinition<B>,
    {
        match ClassSpec::of::<P>() {
            Ok(spec) => self
                .spec
                .push_base(ClassBase::Host(spec)),
            Err(error) => self.reject(error),
        }

        self
    }

    pub fn imported_base(
        &mut self,
        module: impl Into<Cow<'static, str>>,
        qualname: impl Into<Cow<'static, str>>,
    ) -> &mut Self {
        self.spec
            .push_base(ClassBase::Imported {
                module: module.into(),
                qualname: qualname.into(),
            });

        self
    }
}
