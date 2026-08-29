use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use crate::{
    backend::{
        Backend, NativeExtensionLoader, PreparedNativeExtensions, PreparedNativeExtensionsOf, Tok,
    },
    bundle::{Bundle, BundleId},
    catalog::Catalog,
    errors::Error,
    host::module::ModuleSpec,
    imports::name::DottedName,
    native::NativeModule,
};

struct BindingState<B: Backend> {
    sources: HashMap<String, Bundle>,
    prepared: HashMap<BundleId, Rc<PreparedNativeExtensionsOf<B>>>,
    extensions: HashMap<String, Rc<PreparedNativeExtensionsOf<B>>>,
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
    pub(crate) fn new<'py>(
        token: Tok<'py, B>,
        catalog: &Catalog<B>,
        modules: &[Rc<ModuleSpec<B>>],
        natives: &[Rc<NativeModule<B>>],
        bundles: &[Bundle],
        denied: &HashSet<String>,
    ) -> Result<Self, Error> {
        let mut specs = HashMap::new();

        for module in catalog.modules().iter().chain(modules) {
            specs.insert(module.name().to_owned(), module.clone());
        }

        let mut native_specs = HashMap::new();

        for native in catalog.natives().iter().chain(natives) {
            native_specs.insert(native.name().to_owned(), native.clone());

            for alias in native.aliases() {
                native_specs.insert(alias.clone(), native.clone());
            }
        }

        let mut sources = HashMap::new();
        let mut prepared = HashMap::new();
        let mut extensions = HashMap::new();

        for bundle in catalog.bundles().iter().chain(bundles) {
            Self::prepare_bundle(token, bundle, &mut sources, &mut prepared, &mut extensions)?;
        }

        let mut denied_names = catalog.denied().clone();

        denied_names.extend(denied.iter().cloned());

        Ok(Self {
            specs,
            natives: native_specs,
            denied: denied_names,
            state: RefCell::new(BindingState {
                sources,
                prepared,
                extensions,
                realised: HashMap::new(),
                loaded: HashMap::new(),
            }),
        })
    }

    fn prepare_bundle<'py>(
        token: Tok<'py, B>,
        bundle: &Bundle,
        sources: &mut HashMap<String, Bundle>,
        prepared: &mut HashMap<BundleId, Rc<PreparedNativeExtensionsOf<B>>>,
        extensions: &mut HashMap<String, Rc<PreparedNativeExtensionsOf<B>>>,
    ) -> Result<(), Error> {
        let loader = match prepared.get(&bundle.id()) {
            Some(loader) => loader.clone(),
            None => {
                let loader = Rc::new(B::NativeExtensions::prepare(token, bundle)?);

                prepared.insert(bundle.id(), loader.clone());

                loader
            }
        };

        for name in bundle.names() {
            sources.insert(name.to_owned(), bundle.clone());
        }

        for name in loader.names() {
            extensions.insert(name.to_owned(), loader.clone());
        }

        Ok(())
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

    pub(super) fn prepared(&self, id: BundleId) -> Option<Rc<PreparedNativeExtensionsOf<B>>> {
        self.state
            .borrow()
            .prepared
            .get(&id)
            .cloned()
    }

    pub(super) fn extension(&self, dotted: &str) -> Option<Rc<PreparedNativeExtensionsOf<B>>> {
        self.state
            .borrow()
            .extensions
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
            || state.extensions.contains_key(dotted)
            || state.realised.contains_key(dotted)
    }

    pub(super) fn is_host_module(&self, dotted: &str) -> bool {
        self.specs.contains_key(dotted)
    }

    pub(super) fn mount<'py>(
        &self,
        token: Tok<'py, B>,
        bundle: &Bundle,
        root: &str,
    ) -> Result<(), Error> {
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

        let BindingState {
            sources, prepared, extensions, loaded, ..
        } = &mut *state;

        Self::prepare_bundle(token, bundle, sources, prepared, extensions)?;

        loaded.insert(root.to_owned(), bundle.id());

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
            (),
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
        )
        .unwrap();

        assert!(Rc::ptr_eq(&bindings.spec("module").unwrap(), &guest,));
    }

    #[test]
    fn native_aliases_resolve_to_the_same_specification() {
        let native = Rc::new(NativeModule::<Stub>::new("module", StubValue::None).alias("alias"));
        let bindings = GuestBindings::new(
            (),
            &Fixtures::empty(),
            &[],
            &[native.clone()],
            &[],
            &HashSet::new(),
        )
        .unwrap();

        assert!(Rc::ptr_eq(&bindings.native("module").unwrap(), &native,));
        assert!(Rc::ptr_eq(&bindings.native("alias").unwrap(), &native,));
    }

    #[test]
    fn guest_bundles_override_runtime_bundles() {
        let guest = Bundle::single("module", "guest").unwrap();
        let bindings = GuestBindings::new(
            (),
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
        )
        .unwrap();

        assert_eq!(bindings.source("module").unwrap().id(), guest.id(),);
    }

    #[test]
    fn runtime_and_guest_denials_cover_descendants() {
        let bindings = GuestBindings::new(
            (),
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
        )
        .unwrap();

        assert!(bindings.is_denied("runtime.child"));
        assert!(bindings.is_denied("guest.child"));
    }

    #[test]
    fn mounting_the_same_bundle_is_idempotent() {
        let bindings =
            GuestBindings::new((), &Fixtures::empty(), &[], &[], &[], &HashSet::new()).unwrap();
        let bundle = Bundle::single("module", "").unwrap();

        bindings
            .mount((), &bundle, "module")
            .unwrap();
        bindings
            .mount((), &bundle, "module")
            .unwrap();

        assert_eq!(bindings.source("module").unwrap().id(), bundle.id(),);
    }

    #[test]
    fn mounting_a_different_bundle_rejects_an_occupied_root() {
        let bindings =
            GuestBindings::new((), &Fixtures::empty(), &[], &[], &[], &HashSet::new()).unwrap();

        bindings
            .mount((), &Bundle::single("module", "first").unwrap(), "module")
            .unwrap();

        assert!(matches!(
            bindings.mount(
                (),
                &Bundle::single("module", "second").unwrap(),
                "module",
            ),
            Err(Error::NameInUse { ref name }) if name == "module",
        ));
    }

    #[test]
    fn host_module_roots_reject_mounted_bundles() {
        let bindings = GuestBindings::new(
            (),
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
        )
        .unwrap();

        assert!(matches!(
            bindings.mount((), &Bundle::single("module", "").unwrap(), "module"),
            Err(Error::NameInUse { ref name }) if name == "module",
        ));
    }

    fn native_bundle(root: &str, native: &str) -> Bundle {
        Bundle::builder()
            .package(root, "")
            .data(&format!("{root}/{native}.cpython-313-x86_64-linux-gnu.so"), b"".to_vec())
            .build()
            .unwrap()
    }

    #[test]
    fn guest_native_claims_override_runtime_claims() {
        let runtime_bundle = native_bundle("plugin", "native");
        let runtime_id = runtime_bundle.id();
        let guest_bundle = native_bundle("plugin", "native");
        let bindings = GuestBindings::new(
            (),
            &Fixtures::catalog(Vec::new(), Vec::new(), vec![runtime_bundle], HashSet::new()),
            &[],
            &[],
            std::slice::from_ref(&guest_bundle),
            &HashSet::new(),
        )
        .unwrap();

        assert!(Rc::ptr_eq(
            &bindings
                .extension("plugin.native")
                .unwrap(),
            &bindings
                .prepared(guest_bundle.id())
                .unwrap(),
        ));
        assert!(!Rc::ptr_eq(
            &bindings
                .extension("plugin.native")
                .unwrap(),
            &bindings.prepared(runtime_id).unwrap(),
        ));
    }

    #[test]
    fn native_claims_participate_in_contains() {
        let bindings = GuestBindings::new(
            (),
            &Fixtures::empty(),
            &[],
            &[],
            &[native_bundle("plugin", "native")],
            &HashSet::new(),
        )
        .unwrap();

        assert!(bindings.contains("plugin.native"));
    }

    #[test]
    fn reuses_preparation_for_clones_of_one_bundle() {
        let bundle = native_bundle("plugin", "native");
        let bindings = GuestBindings::new(
            (),
            &Fixtures::catalog(Vec::new(), Vec::new(), vec![bundle.clone()], HashSet::new()),
            &[],
            &[],
            std::slice::from_ref(&bundle),
            &HashSet::new(),
        )
        .unwrap();

        assert!(Rc::ptr_eq(
            &bindings.prepared(bundle.id()).unwrap(),
            &bindings
                .extension("plugin.native")
                .unwrap(),
        ));
    }

    #[test]
    fn mounting_prepares_and_indexes_native_claims() {
        let bindings =
            GuestBindings::new((), &Fixtures::empty(), &[], &[], &[], &HashSet::new()).unwrap();

        bindings
            .mount((), &native_bundle("plugin", "native"), "plugin")
            .unwrap();

        assert!(
            bindings
                .extension("plugin.native")
                .is_some()
        );
    }

    #[test]
    fn remounting_the_same_bundle_does_not_prepare_twice() {
        let bindings =
            GuestBindings::new((), &Fixtures::empty(), &[], &[], &[], &HashSet::new()).unwrap();
        let bundle = native_bundle("plugin", "native");

        bindings
            .mount((), &bundle, "plugin")
            .unwrap();

        let first = bindings.prepared(bundle.id()).unwrap();

        bindings
            .mount((), &bundle, "plugin")
            .unwrap();

        let second = bindings.prepared(bundle.id()).unwrap();

        assert!(Rc::ptr_eq(&first, &second));
    }

    #[test]
    fn occupied_roots_fail_before_preparation() {
        let bindings = GuestBindings::new(
            (),
            &Fixtures::catalog(
                vec![Rc::new(ModuleSpec::<Stub>::new("plugin"))],
                Vec::new(),
                Vec::new(),
                HashSet::new(),
            ),
            &[],
            &[],
            &[],
            &HashSet::new(),
        )
        .unwrap();

        assert!(matches!(
            bindings.mount((), &native_bundle("plugin", "native"), "plugin"),
            Err(Error::NameInUse { ref name }) if name == "plugin",
        ));
        assert!(
            bindings
                .extension("plugin.native")
                .is_none()
        );
    }
}
