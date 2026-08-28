use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use crate::{
    backend::Backend,
    bundle::{Bundle, BundleId},
    catalog::Catalog,
    errors::Error,
    host::module::ModuleSpec,
    imports::name::DottedName,
    native::NativeModule,
};

struct BindingState<B: Backend> {
    sources: HashMap<String, Bundle>,
    realised: HashMap<String, B::Owned>,
    loaded: HashMap<String, BundleId>,
}

pub(crate) struct GuestBindings<B: Backend> {
    specs: HashMap<String, Rc<ModuleSpec<B>>>,
    natives: HashMap<String, Rc<NativeModule<B>>>,
    denied: HashSet<String>,
    state: RefCell<BindingState<B>>,
}

impl<B: Backend> GuestBindings<B> {
    pub(crate) fn new(
        catalog: &Catalog<B>,
        modules: &[Rc<ModuleSpec<B>>],
        natives: &[Rc<NativeModule<B>>],
        bundles: &[Bundle],
        denied: &HashSet<String>,
    ) -> Self {
        let mut specs = HashMap::new();

        for module in catalog
            .modules()
            .iter()
            .chain(modules)
        {
            specs.insert(module.name().to_owned(), module.clone());
        }

        let mut native_specs = HashMap::new();

        for native in catalog
            .natives()
            .iter()
            .chain(natives)
        {
            native_specs.insert(native.name().to_owned(), native.clone());

            for alias in native.aliases() {
                native_specs.insert(alias.clone(), native.clone());
            }
        }

        let mut sources = HashMap::new();

        for bundle in catalog
            .bundles()
            .iter()
            .chain(bundles)
        {
            for name in bundle.names() {
                sources.insert(name.to_owned(), bundle.clone());
            }
        }

        let mut denied_names = catalog.denied().clone();

        denied_names.extend(denied.iter().cloned());

        Self {
            specs,
            natives: native_specs,
            denied: denied_names,
            state: RefCell::new(BindingState {
                sources,
                realised: HashMap::new(),
                loaded: HashMap::new(),
            }),
        }
    }

    pub(super) fn spec(&self, dotted: &str) -> Option<Rc<ModuleSpec<B>>> {
        self.specs.get(dotted).cloned()
    }

    pub(super) fn native(&self, dotted: &str) -> Option<Rc<NativeModule<B>>> {
        self.natives.get(dotted).cloned()
    }

    pub(super) fn source(&self, dotted: &str) -> Option<Bundle> {
        self.state
            .borrow()
            .sources
            .get(dotted)
            .cloned()
    }

    pub(super) fn cached(&self, dotted: &str) -> Option<B::Owned> {
        self.state
            .borrow()
            .realised
            .get(dotted)
            .cloned()
    }

    pub(super) fn cache(&self, dotted: &str, module: B::Owned) {
        self.state
            .borrow_mut()
            .realised
            .insert(dotted.to_owned(), module);
    }

    pub(super) fn remove_cached(&self, dotted: &str) -> Option<B::Owned> {
        self.state
            .borrow_mut()
            .realised
            .remove(dotted)
    }

    pub(super) fn is_denied(&self, dotted: &str) -> bool {
        DottedName(dotted).is_denied(&self.denied)
    }

    pub(super) fn contains(&self, dotted: &str) -> bool {
        let state = self.state.borrow();

        self.specs.contains_key(dotted)
            || self.natives.contains_key(dotted)
            || state.sources.contains_key(dotted)
            || state.realised.contains_key(dotted)
    }

    pub(super) fn is_host_module(&self, dotted: &str) -> bool {
        self.specs.contains_key(dotted)
    }

    pub(super) fn mount(&self, bundle: &Bundle, root: &str) -> Result<(), Error> {
        if self.specs.contains_key(root) {
            return Err(Error::NameInUse { name: root.to_owned() });
        }

        let mut state = self.state.borrow_mut();

        match state.loaded.get(root) {
            Some(id) if *id == bundle.id() => return Ok(()),
            Some(_) => {
                return Err(Error::NameInUse { name: root.to_owned() });
            }
            None => {}
        }

        for name in bundle.names() {
            state
                .sources
                .insert(name.to_owned(), bundle.clone());
        }

        state
            .loaded
            .insert(root.to_owned(), bundle.id());

        Ok(())
    }
}

impl<B: Backend> Drop for GuestBindings<B> {
    fn drop(&mut self) {
        for value in self
            .state
            .get_mut()
            .realised
            .drain()
            .map(|(_, value)| value)
        {
            B::release(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, rc::Rc};

    use super::GuestBindings;
    use crate::{
        backend::tests::{Stub, StubValue},
        bundle::Bundle,
        catalog::Catalog,
        errors::Error,
        host::module::ModuleSpec,
        native::NativeModule,
    };

    struct Fixtures;

    impl Fixtures {
        fn catalog(
            modules: Vec<Rc<ModuleSpec<Stub>>>,
            natives: Vec<Rc<NativeModule<Stub>>>,
            bundles: Vec<Bundle>,
            denied: HashSet<String>,
        ) -> Catalog<Stub> {
            Catalog::new(modules, natives, bundles, denied, Vec::new(), Vec::new())
        }

        fn empty() -> Catalog<Stub> {
            Self::catalog(Vec::new(), Vec::new(), Vec::new(), HashSet::new())
        }
    }

    #[test]
    fn guest_host_modules_override_runtime_modules() {
        let guest = Rc::new(ModuleSpec::<Stub>::new("module"));
        let bindings = GuestBindings::new(
            &Fixtures::catalog(
                vec![Rc::new(ModuleSpec::<Stub>::new("module"))],
                Vec::new(),
                Vec::new(),
                HashSet::new(),
            ),
            &[guest.clone()],
            &[],
            &[],
            &HashSet::new(),
        );

        assert!(Rc::ptr_eq(&bindings.spec("module").unwrap(), &guest,));
    }

    #[test]
    fn native_aliases_resolve_to_the_same_specification() {
        let native = Rc::new(NativeModule::<Stub>::new("module", StubValue::None).alias("alias"));
        let bindings =
            GuestBindings::new(&Fixtures::empty(), &[], &[native.clone()], &[], &HashSet::new());

        assert!(Rc::ptr_eq(&bindings.native("module").unwrap(), &native,));
        assert!(Rc::ptr_eq(&bindings.native("alias").unwrap(), &native,));
    }

    #[test]
    fn guest_bundles_override_runtime_bundles() {
        let guest = Bundle::single("module", "guest").unwrap();
        let bindings = GuestBindings::new(
            &Fixtures::catalog(
                Vec::new(),
                Vec::new(),
                vec![Bundle::single("module", "runtime").unwrap()],
                HashSet::new(),
            ),
            &[],
            &[],
            std::slice::from_ref(&guest),
            &HashSet::new(),
        );

        assert_eq!(bindings.source("module").unwrap().id(), guest.id(),);
    }

    #[test]
    fn runtime_and_guest_denials_cover_descendants() {
        let bindings = GuestBindings::new(
            &Fixtures::catalog(
                Vec::new(),
                Vec::new(),
                Vec::new(),
                HashSet::from(["runtime".to_owned()]),
            ),
            &[],
            &[],
            &[],
            &HashSet::from(["guest".to_owned()]),
        );

        assert!(bindings.is_denied("runtime.child"));
        assert!(bindings.is_denied("guest.child"));
    }

    #[test]
    fn mounting_the_same_bundle_is_idempotent() {
        let bindings = GuestBindings::new(&Fixtures::empty(), &[], &[], &[], &HashSet::new());
        let bundle = Bundle::single("module", "").unwrap();

        bindings
            .mount(&bundle, "module")
            .unwrap();
        bindings
            .mount(&bundle, "module")
            .unwrap();

        assert_eq!(bindings.source("module").unwrap().id(), bundle.id(),);
    }

    #[test]
    fn mounting_a_different_bundle_rejects_an_occupied_root() {
        let bindings = GuestBindings::new(&Fixtures::empty(), &[], &[], &[], &HashSet::new());

        bindings
            .mount(&Bundle::single("module", "first").unwrap(), "module")
            .unwrap();

        assert!(matches!(
            bindings.mount(
                &Bundle::single("module", "second").unwrap(),
                "module",
            ),
            Err(Error::NameInUse { ref name }) if name == "module",
        ));
    }

    #[test]
    fn host_module_roots_reject_mounted_bundles() {
        let bindings = GuestBindings::new(
            &Fixtures::catalog(
                vec![Rc::new(ModuleSpec::<Stub>::new("module"))],
                Vec::new(),
                Vec::new(),
                HashSet::new(),
            ),
            &[],
            &[],
            &[],
            &HashSet::new(),
        );

        assert!(matches!(
            bindings.mount(
                &Bundle::single("module", "").unwrap(),
                "module",
            ),
            Err(Error::NameInUse { ref name }) if name == "module",
        ));
    }
}
