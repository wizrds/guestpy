use std::{
    any::TypeId, cell::RefCell, collections::HashMap, future::Future, marker::PhantomData, rc::Rc,
};

use crate::{
    backend::{
        Backend, BackendCallables, BackendClasses, BackendCoroutines, BackendExceptions,
        BackendModules, BackendValues, Val,
        callables::{HostBody, PendingValue, RawBody},
    },
    errors::Error,
    handle::Value,
    host::{
        declaration::{DeclarationContext, DeclareMember, Member},
        dunder::Dunder,
        namespace::{Namespace, ValueDeclaration},
    },
    marshal::{FromGuest, FromGuestMut, FromGuestRef, ToGuest, args::Args},
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

pub(crate) enum MemberName {
    Named(String),
    Dunder(Dunder),
}

struct MethodDeclaration<B: Backend> {
    body: MethodBody<B>,
}

impl<B: Backend> MethodDeclaration<B> {
    fn new(body: MethodBody<B>) -> Self {
        Self { body }
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
    fn new(body: MethodBody<B>) -> Self {
        Self { body }
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
    fn new(body: HostBody<B>) -> Self {
        Self { body }
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

    fn set_get(&self, get: MethodBody<B>) {
        *self.get.borrow_mut() = Some(get);
    }

    fn set_set(&self, set: SetterBody<B>) {
        *self.set.borrow_mut() = Some(set);
    }

    fn set_del(&self, del: DeleterBody<B>) {
        *self.del.borrow_mut() = Some(del);
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

pub struct ClassSpec<B: Backend> {
    name: &'static str,
    doc: Option<&'static str>,
    module: RefCell<Option<String>>,
    bases: Vec<Rc<ClassSpec<B>>>,
    alloc: AllocBody<B>,
    init: InitBody<B>,
    members: Vec<(MemberName, Member<B>)>,
    statics: Namespace<B>,
    payload: TypeId,
}

impl<B: Backend> ClassSpec<B> {
    pub(crate) fn payload(&self) -> TypeId {
        self.payload
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

    pub(crate) fn bases(&self) -> &[Rc<ClassSpec<B>>] {
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
    pub(crate) fn of<C>() -> Rc<ClassSpec<B>>
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

        Rc::new(builder.spec)
    }
}

struct ClassRealiser<'py, 'e, B: Backend> {
    enter: &'e Enter<'py, B>,
}

impl<'py, 'e, B> ClassRealiser<'py, 'e, B>
where
    B: Backend + BackendValues + BackendCallables + BackendClasses,
{
    fn new(enter: &'e Enter<'py, B>) -> Self {
        Self { enter }
    }

    fn class_new(&self, spec: &Rc<ClassSpec<B>>) -> Result<Val<'py, B>, Error> {
        let alloc = spec.alloc().clone();

        B::function(
            self.enter.token(),
            "__new__",
            None,
            self.enter
                .guest()
                .raw_body(Rc::new(move |enter, args| alloc(enter, args.split_receiver()?.0))),
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

    fn realise(&self, spec: &Rc<ClassSpec<B>>) -> Result<Val<'py, B>, Error> {
        let realisation = self.enter.guest().realisation();
        let payload = spec.payload();

        if !realisation.class_registered(payload) {
            return Err(Error::unexpected("host class was not registered"));
        }

        if let Some(owned) = realisation.realised_class(payload) {
            return Ok(B::attach(self.enter.token(), &owned));
        }

        let mut bases = Vec::new();

        for base in spec.bases() {
            bases.push(self.realise(base)?);
        }

        let context = DeclarationContext::new(self.enter);
        let namespace = B::new_dict(self.enter.token())?;

        for (member_name, member) in spec.members() {
            let attribute = match member_name {
                MemberName::Named(name) => name.clone(),
                MemberName::Dunder(dunder) => dunder.name().to_owned(),
            };
            let value = member.realise(&context, &attribute)?;

            B::set_item(
                self.enter.token(),
                &namespace,
                B::str(self.enter.token(), &attribute),
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

        let class = B::new_class(self.enter.token(), spec.name(), &bases, &namespace)?;

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
    B: Backend + BackendValues + BackendCallables + BackendClasses,
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

    fn construct<'py, B>(_enter: &Enter<'py, B>, _args: Args<'py, B>) -> Result<Self, Error>
    where
        B: Backend + BackendValues + BackendCallables + BackendClasses,
    {
        Err(Error::unsupported(format!("host class {} cannot be constructed", Self::NAME,)))
    }
}

pub trait HostClassDefinition<B: Backend>: HostClass {
    fn build(builder: &mut ClassBuilder<B, Self>);
}

pub struct ClassBuilder<B: Backend, C> {
    spec: ClassSpec<B>,
    properties: HashMap<String, Rc<ClassPropertyDeclaration<B>>>,
    marker: PhantomData<fn() -> C>,
}

impl<B, C> ClassBuilder<B, C>
where
    B: Backend + BackendValues + BackendCallables + BackendClasses,
    C: HostClass,
{
    fn new() -> Self {
        Self {
            spec: ClassSpec {
                name: C::NAME,
                doc: C::DOC,
                module: RefCell::new(None),
                bases: Vec::new(),
                alloc: Rc::new(|enter, class| B::alloc::<C>(enter.token(), &class)),
                init: Rc::new(|enter, instance, args| {
                    B::set_payload::<C>(enter.token(), &instance, C::construct(enter, args)?)
                }),
                members: Vec::new(),
                statics: Namespace::new(),
                payload: TypeId::of::<C>(),
            },
            properties: HashMap::new(),
            marker: PhantomData,
        }
    }

    fn push(&mut self, name: &str, member: Member<B>) -> &mut Self {
        self.spec
            .members
            .push((MemberName::Named(name.to_owned()), member));

        self
    }

    fn property_slot(&mut self, name: &str) -> Rc<ClassPropertyDeclaration<B>> {
        if let Some(property) = self.properties.get(name) {
            return property.clone();
        }

        let property = Rc::new(ClassPropertyDeclaration::new());

        self.properties
            .insert(name.to_owned(), property.clone());
        self.push(name, property.clone());

        property
    }

    pub fn method<F, R>(&mut self, name: &str, function: F) -> &mut Self
    where
        F: for<'py> Fn(&C, &Enter<'py, B>, Args<'py, B>) -> Result<R, Error> + 'static,
        R: ToGuest<B> + 'static,
    {
        self.push(
            name,
            Rc::new(MethodDeclaration::new(Rc::new(move |enter, receiver, args| {
                function(&*C::from_guest_ref(enter, &receiver)?, enter, args)?.to_guest(enter)
            }))),
        )
    }

    pub fn method_mut<F, R>(&mut self, name: &str, function: F) -> &mut Self
    where
        F: for<'py> Fn(&mut C, &Enter<'py, B>, Args<'py, B>) -> Result<R, Error> + 'static,
        R: ToGuest<B> + 'static,
    {
        self.push(
            name,
            Rc::new(MethodDeclaration::new(Rc::new(move |enter, receiver, args| {
                function(&mut *C::from_guest_mut(enter, &receiver)?, enter, args)?.to_guest(enter)
            }))),
        )
    }

    pub fn class_method<F, R>(&mut self, name: &str, function: F) -> &mut Self
    where
        F: for<'py> Fn(&Enter<'py, B>, B::Value<'py>, Args<'py, B>) -> Result<R, Error> + 'static,
        R: ToGuest<B> + 'static,
    {
        self.push(
            name,
            Rc::new(ClassMethodDeclaration::new(Rc::new(move |enter, class, args| {
                function(enter, class, args)?.to_guest(enter)
            }))),
        )
    }

    pub fn static_method<F, R>(&mut self, name: &str, function: F) -> &mut Self
    where
        F: for<'py> Fn(&Enter<'py, B>, Args<'py, B>) -> Result<R, Error> + 'static,
        R: ToGuest<B> + 'static,
    {
        self.push(
            name,
            Rc::new(StaticMethodDeclaration::new(Rc::new(move |enter, args| {
                function(enter, args)?.to_guest(enter)
            }))),
        )
    }

    pub fn getter<F, R>(&mut self, name: &str, get: F) -> &mut Self
    where
        F: for<'py> Fn(&C, &Enter<'py, B>) -> Result<R, Error> + 'static,
        R: ToGuest<B> + 'static,
    {
        self.property_slot(name)
            .set_get(Rc::new(move |enter, receiver, _| {
                get(&*C::from_guest_ref(enter, &receiver)?, enter)?.to_guest(enter)
            }));

        self
    }

    pub fn setter<F, V>(&mut self, name: &str, set: F) -> &mut Self
    where
        F: for<'py> Fn(&mut C, &Enter<'py, B>, V) -> Result<(), Error> + 'static,
        V: FromGuest<B, Owned = V> + 'static,
    {
        self.property_slot(name)
            .set_set(Rc::new(move |enter, receiver, value| {
                set(&mut *C::from_guest_mut(enter, &receiver)?, enter, V::from_guest(enter, value)?)
            }));

        self
    }

    pub fn deleter<F>(&mut self, name: &str, del: F) -> &mut Self
    where
        F: for<'py> Fn(&mut C, &Enter<'py, B>) -> Result<(), Error> + 'static,
    {
        self.property_slot(name)
            .set_del(Rc::new(move |enter, receiver| {
                del(&mut *C::from_guest_mut(enter, &receiver)?, enter)
            }));

        self
    }

    pub fn property<G, S, R, V>(&mut self, name: &str, get: G, set: S) -> &mut Self
    where
        G: for<'py> Fn(&C, &Enter<'py, B>) -> Result<R, Error> + 'static,
        S: for<'py> Fn(&mut C, &Enter<'py, B>, V) -> Result<(), Error> + 'static,
        R: ToGuest<B> + 'static,
        V: FromGuest<B, Owned = V> + 'static,
    {
        self.getter(name, get).setter(name, set)
    }

    pub fn constant<V>(&mut self, name: &str, value: V) -> &mut Self
    where
        V: ToGuest<B> + Clone + 'static,
    {
        self.push(name, Rc::new(ValueDeclaration::new(Namespace::constant_thunk(value))))
    }

    pub fn dunder<F, R>(&mut self, dunder: Dunder, function: F) -> &mut Self
    where
        F: for<'py> Fn(&C, &Enter<'py, B>, Args<'py, B>) -> Result<R, Error> + 'static,
        R: ToGuest<B> + 'static,
    {
        self.spec.members.push((
            MemberName::Dunder(dunder),
            Rc::new(MethodDeclaration::new(Rc::new(move |enter, receiver, args| {
                function(&*C::from_guest_ref(enter, &receiver)?, enter, args)?.to_guest(enter)
            }))),
        ));

        self
    }

    pub fn statics<F: FnOnce(&mut Namespace<B>)>(&mut self, build: F) -> &mut Self {
        build(&mut self.spec.statics);

        self
    }
}

impl<B, C> ClassBuilder<B, C>
where
    B: Backend
        + BackendValues
        + BackendCallables
        + BackendClasses
        + BackendModules
        + BackendCoroutines
        + BackendExceptions,
    C: HostClass,
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

    pub fn async_method<F, Fut, R>(&mut self, name: &str, function: F) -> &mut Self
    where
        F: for<'py> Fn(&C, &Enter<'py, B>, Args<'py, B>) -> Result<Fut, Error> + 'static,
        Fut: Future<Output = Result<R, Error>> + 'static,
        R: ToGuest<B> + 'static,
    {
        self.push(
            name,
            Rc::new(MethodDeclaration::new(Rc::new(move |enter, receiver, args| {
                Self::pending(enter, function(&*C::from_guest_ref(enter, &receiver)?, enter, args)?)
            }))),
        )
    }
}

impl<B, C> ClassBuilder<B, C>
where
    B: Backend + BackendValues + BackendCallables + BackendClasses,
    C: HostClass,
{
    pub fn base<P>(&mut self) -> &mut Self
    where
        P: HostClass + HostClassDefinition<B>,
    {
        self.spec
            .bases
            .push(ClassSpec::of::<P>());

        self
    }
}
