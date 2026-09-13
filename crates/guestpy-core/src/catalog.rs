//! Runtime-internal declarations.

use std::{
    any::TypeId,
    cell::RefCell,
    collections::{HashMap, HashSet},
    hash::Hash,
    rc::Rc,
};

use crate::{
    backend::Backend,
    bundle::{Bundle, BundleId},
    host::{
        class::ClassSpec,
        exception::{ExceptionKey, ExceptionSpec},
        library::HostInitializer,
        module::ModuleSpec,
    },
    native::{NativeInitializer, NativeModule},
};

struct InternedEntry<B: Backend, V> {
    spec: Rc<V>,
    realised: Option<B::Owned>,
}

struct Interned<B: Backend, K, V> {
    entries: RefCell<HashMap<K, InternedEntry<B, V>>>,
}

impl<B: Backend, K, V> Interned<B, K, V>
where
    K: Eq + Hash,
{
    fn new() -> Self {
        Self { entries: RefCell::new(HashMap::new()) }
    }

    fn intern(&self, key: K, spec: Rc<V>) {
        self.entries
            .borrow_mut()
            .entry(key)
            .or_insert(InternedEntry { spec, realised: None });
    }

    fn contains(&self, key: &K) -> bool {
        self.entries.borrow().contains_key(key)
    }

    fn spec(&self, key: &K) -> Option<Rc<V>> {
        self.entries
            .borrow()
            .get(key)
            .map(|entry| entry.spec.clone())
    }

    fn realised(&self, key: &K) -> Option<B::Owned> {
        self.entries
            .borrow()
            .get(key)
            .and_then(|entry| entry.realised.clone())
    }

    fn set_realised(&self, key: &K, owned: B::Owned) {
        self.entries
            .borrow_mut()
            .get_mut(key)
            .expect("declaration registered")
            .realised = Some(owned);
    }
}

pub(crate) struct Catalog<B: Backend> {
    modules: Vec<Rc<ModuleSpec<B>>>,
    natives: Vec<Rc<NativeModule<B>>>,
    bundles: Vec<Bundle>,
    denied: HashSet<String>,
    host_initializers: Vec<HostInitializer<B>>,
    native_initializers: Vec<NativeInitializer<B>>,
}

impl<B: Backend> Catalog<B> {
    pub(crate) fn new(
        modules: Vec<Rc<ModuleSpec<B>>>,
        natives: Vec<Rc<NativeModule<B>>>,
        bundles: Vec<Bundle>,
        denied: HashSet<String>,
        host_initializers: Vec<HostInitializer<B>>,
        native_initializers: Vec<NativeInitializer<B>>,
    ) -> Self {
        Self {
            modules,
            natives,
            bundles,
            denied,
            host_initializers,
            native_initializers,
        }
    }

    pub(crate) fn modules(&self) -> &[Rc<ModuleSpec<B>>] {
        &self.modules
    }

    pub(crate) fn natives(&self) -> &[Rc<NativeModule<B>>] {
        &self.natives
    }

    pub(crate) fn bundles(&self) -> &[Bundle] {
        &self.bundles
    }

    pub(crate) fn denied(&self) -> &HashSet<String> {
        &self.denied
    }

    pub(crate) fn host_initializers(&self) -> &[HostInitializer<B>] {
        &self.host_initializers
    }

    pub(crate) fn native_initializers(&self) -> &[NativeInitializer<B>] {
        &self.native_initializers
    }
}

pub(crate) struct RealisationCache<B: Backend> {
    classes: Interned<B, TypeId, ClassSpec<B>>,
    exceptions: Interned<B, ExceptionKey, ExceptionSpec>,
    code: RefCell<HashMap<(BundleId, String), B::Owned>>,
}

impl<B: Backend> RealisationCache<B> {
    pub(crate) fn new() -> Self {
        Self {
            classes: Interned::new(),
            exceptions: Interned::new(),
            code: RefCell::new(HashMap::new()),
        }
    }

    fn absorb_class(&self, spec: &Rc<ClassSpec<B>>) {
        let payload = spec.payload();

        if self.classes.contains(&payload) {
            return;
        }

        self.classes
            .intern(payload, spec.clone());

        for base in spec.bases() {
            self.absorb_class(base);
        }
    }

    pub(crate) fn absorb(&self, module: &ModuleSpec<B>) {
        for spec in module.classes() {
            self.absorb_class(spec);
        }

        for spec in module.exceptions() {
            self.exceptions
                .intern(spec.key().clone(), spec.clone());
        }
    }

    pub(crate) fn compiled(&self, bundle: BundleId, dotted: &str) -> Option<B::Owned> {
        self.code
            .borrow()
            .get(&(bundle, dotted.to_owned()))
            .cloned()
    }

    pub(crate) fn cache_compiled(&self, bundle: BundleId, dotted: &str, code: B::Owned) {
        self.code
            .borrow_mut()
            .insert((bundle, dotted.to_owned()), code);
    }

    pub(crate) fn class_registered(&self, payload: TypeId) -> bool {
        self.classes.contains(&payload)
    }

    pub(crate) fn class_spec(&self, payload: TypeId) -> Option<Rc<ClassSpec<B>>> {
        self.classes.spec(&payload)
    }

    pub(crate) fn realised_class(&self, payload: TypeId) -> Option<B::Owned> {
        self.classes.realised(&payload)
    }

    pub(crate) fn set_realised_class(&self, payload: TypeId, owned: B::Owned) {
        self.classes
            .set_realised(&payload, owned);
    }

    pub(crate) fn realised_exception(&self, key: &ExceptionKey) -> Option<B::Owned> {
        self.exceptions.realised(key)
    }

    pub(crate) fn set_realised_exception(&self, key: &ExceptionKey, owned: B::Owned) {
        self.exceptions.set_realised(key, owned);
    }

    pub(crate) fn exception_spec(&self, key: &ExceptionKey) -> Option<Rc<ExceptionSpec>> {
        self.exceptions.spec(key)
    }
}

#[cfg(test)]
mod tests {
    use std::{any::TypeId, rc::Rc};

    use super::{Interned, RealisationCache};
    use crate::{
        backend::tests::Stub,
        host::{
            exception::{ExceptionKey, HostException},
            module::ModuleSpec,
        },
    };

    struct Example;

    impl HostException for Example {
        const NAME: &'static str = "Example";
    }

    #[test]
    fn interned_keeps_the_first_specification() {
        let interned = Interned::<Stub, _, _>::new();

        interned.intern("entry", Rc::new("first"));
        interned.intern("entry", Rc::new("second"));

        assert_eq!(interned.spec(&"entry").as_deref(), Some(&"first"),);
    }

    #[test]
    fn interned_pairs_realisation_with_its_key() {
        let interned = Interned::<Stub, _, _>::new();

        interned.intern("first", Rc::new("first"));
        interned.intern("second", Rc::new("second"));
        interned.set_realised(&"second", ());

        assert!(interned.realised(&"first").is_none());
        assert!(interned.realised(&"second").is_some());
    }

    #[test]
    fn exceptions_keep_their_first_registration() {
        let realisation = RealisationCache::<Stub>::new();
        let key = ExceptionKey::Typed {
            id: TypeId::of::<Example>(),
            name: Example::NAME,
        };

        realisation.absorb(&ModuleSpec::new("mod_a").exception_type::<Example>());
        realisation.set_realised_exception(&key, ());
        realisation.absorb(&ModuleSpec::new("mod_b").exception_type::<Example>());

        assert_eq!(
            realisation
                .exception_spec(&key)
                .map(|spec| spec.module().to_owned())
                .as_deref(),
            Some("mod_a"),
        );
        assert!(realisation
            .realised_exception(&key)
            .is_some());
    }
}
