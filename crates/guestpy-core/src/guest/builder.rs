use std::{collections::HashSet, rc::Rc, time::Duration};

use crate::{
    backend::{Backend, BackendCallables, BackendLibrary, BackendModules, BackendValues},
    bundle::Bundle,
    errors::Error,
    guest::{ActiveGuest, Guest, GuestId, GuestInner},
    host::{
        library::{HostInitializer, HostLibrary, HostLibraryEntry},
        module::ModuleSpec,
    },
    imports::{GuestBindings, Imports},
    native::{NativeInitializer, NativeLibrary, NativeLibraryEntry, NativeModule},
    runtime::Runtime,
    scope::Enter,
};

pub struct GuestBuilder<'r, B: Backend> {
    runtime: &'r Runtime<B>,
    modules: Vec<Rc<ModuleSpec<B>>>,
    natives: Vec<Rc<NativeModule<B>>>,
    bundles: Vec<Bundle>,
    denied: HashSet<String>,
    host_initializers: Vec<HostInitializer<B>>,
    native_initializers: Vec<NativeInitializer<B>>,
    timeout: Option<Duration>,
}

impl<'r, B: Backend> GuestBuilder<'r, B> {
    pub(crate) fn new(runtime: &'r Runtime<B>) -> Self {
        Self {
            runtime,
            modules: Vec::new(),
            natives: Vec::new(),
            bundles: Vec::new(),
            denied: HashSet::new(),
            host_initializers: Vec::new(),
            native_initializers: Vec::new(),
            timeout: None,
        }
    }

    pub fn bind(mut self, library: impl Into<HostLibrary<B>>) -> Self {
        for entry in library.into().into_entries() {
            match entry {
                HostLibraryEntry::Module(module) => self.modules.push(module),
                HostLibraryEntry::Initializer(initializer) => {
                    self.host_initializers.push(initializer)
                }
            }
        }

        self
    }

    pub fn bundle(mut self, bundle: Bundle) -> Self {
        self.bundles.push(bundle);

        self
    }

    pub fn deny(mut self, name: &str) -> Self {
        self.denied.insert(name.to_owned());

        self
    }

    pub fn timeout(mut self, budget: Duration) -> Self {
        self.timeout = Some(budget);

        self
    }
}

impl<'r, B> GuestBuilder<'r, B>
where
    B: Backend + BackendLibrary,
{
    pub fn bind_native(mut self, library: impl Into<NativeLibrary<B>>) -> Self {
        for entry in library.into().into_entries() {
            match entry {
                NativeLibraryEntry::Module(module) => self.natives.push(Rc::new(module)),
                NativeLibraryEntry::Initializer(initializer) => self
                    .native_initializers
                    .push(initializer),
            }
        }

        self
    }
}

impl<'r, B> GuestBuilder<'r, B>
where
    B: Backend + BackendValues + BackendCallables + BackendModules,
{
    pub fn build(self) -> Result<Guest<B>, Error> {
        let runtime = &self.runtime.inner;
        let id = GuestId::new(runtime.take_next_id());

        for module in &self.modules {
            runtime
                .realisation()
                .absorb(module);
        }

        B::enter(runtime.engine(), |token| {
            let bindings = GuestBindings::new(
                token,
                runtime.catalog(),
                &self.modules,
                &self.natives,
                &self.bundles,
                &self.denied,
            )?;
            let globals = B::new_dict(token)?;
            let builtins = B::copy_dict(token, &B::builtins_dict(token)?)?;

            B::set_item(token, &globals, B::str(token, "__builtins__"), builtins.clone())?;
            B::set_item(token, &globals, B::str(token, "__name__"), B::str(token, "__main__"))?;
            B::set_item(
                token,
                &globals,
                B::str(token, "__guestpy_id__"),
                B::uint(token, id.value()),
            )?;

            let guest = Guest {
                inner: Rc::new(GuestInner::new(
                    id,
                    runtime.clone(),
                    B::new_context(token, globals, builtins),
                    runtime.policy().derive(self.timeout),
                    bindings,
                )),
            };

            runtime
                .registry()
                .register(&guest.inner);

            B::set_item(
                token,
                &B::context_builtins(token, guest.context()),
                B::str(token, "__import__"),
                B::function(
                    token,
                    "__import__",
                    None,
                    guest.raw_body(Rc::new(|enter, args| Imports::new(enter).dispatch(&args))),
                )?,
            )?;

            let _active = ActiveGuest::operation(&guest.inner)?;

            runtime
                .catalog()
                .modules()
                .iter()
                .chain(&self.modules)
                .filter_map(|module| module.init_hook())
                .try_for_each(|init| init(&Enter::new(token, guest.clone())))?;

            runtime
                .catalog()
                .host_initializers()
                .iter()
                .chain(&self.host_initializers)
                .try_for_each(|initializer| initializer.run(&Enter::new(token, guest.clone())))?;

            runtime
                .catalog()
                .native_initializers()
                .iter()
                .chain(&self.native_initializers)
                .try_for_each(|initializer| initializer.run(&Enter::new(token, guest.clone())))?;

            Ok(guest)
        })
    }
}
