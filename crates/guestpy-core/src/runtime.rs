//! Runtime construction and ownership.

use std::{cell::Cell, collections::HashSet, rc::Rc, time::Duration};

use crate::{
    backend::{Backend, BackendCallables, BackendLibrary, BackendModules, BackendValues},
    bundle::Bundle,
    catalog::{Catalog, RealisationCache},
    errors::Error,
    guest::{GuestBuilder, GuestInner, GuestRegistry},
    host::{
        library::{HostInitializer, HostLibrary, HostLibraryEntry},
        module::ModuleSpec,
    },
    native::{NativeInitializer, NativeLibrary, NativeLibraryEntry, NativeModule},
    policy::{CancelSignal, ExecutionPolicy},
};

pub(crate) struct RuntimeInner<B: Backend> {
    engine: B::Engine,
    catalog: Catalog<B>,
    realisation: RealisationCache<B>,
    registry: GuestRegistry<B>,
    next_id: Cell<u64>,
    policy: ExecutionPolicy,
    real_import: B::Owned,
    release: Cell<bool>,
}

impl<B: Backend> RuntimeInner<B> {
    pub(crate) fn engine(&self) -> &B::Engine {
        &self.engine
    }

    pub(crate) fn real_import(&self) -> &B::Owned {
        &self.real_import
    }

    pub(crate) fn catalog(&self) -> &Catalog<B> {
        &self.catalog
    }

    pub(crate) fn realisation(&self) -> &RealisationCache<B> {
        &self.realisation
    }

    pub(crate) fn registry(&self) -> &GuestRegistry<B> {
        &self.registry
    }

    pub(crate) fn policy(&self) -> &ExecutionPolicy {
        &self.policy
    }

    pub(crate) fn take_next_id(&self) -> u64 {
        let id = self.next_id.get();

        self.next_id.set(
            id.checked_add(1)
                .expect("guest ID counter overflowed"),
        );

        id
    }
}

pub struct Runtime<B: Backend> {
    pub(crate) inner: Rc<RuntimeInner<B>>,
}

impl<B: Backend> Clone for Runtime<B> {
    fn clone(&self) -> Self {
        Self { inner: self.inner.clone() }
    }
}

impl<B: Backend> Runtime<B> {
    pub fn builder() -> RuntimeBuilder<B> {
        RuntimeBuilder {
            modules: Vec::new(),
            natives: Vec::new(),
            bundles: Vec::new(),
            denied: HashSet::new(),
            host_initializers: Vec::new(),
            native_initializers: Vec::new(),
            config: B::Config::default(),
            policy: ExecutionPolicy::new(),
        }
    }

    pub fn guest(&self) -> GuestBuilder<'_, B> {
        GuestBuilder::new(self)
    }

    pub fn shutdown(self) -> Result<(), Error> {
        let inner = Rc::try_unwrap(self.inner)
            .map_err(|_| Error::unexpected("cannot shut down runtime while guests are live"))?;

        if inner.release.replace(true) {
            return Ok(());
        }

        B::shutdown(inner.engine)
    }
}

pub struct RuntimeBuilder<B: Backend> {
    modules: Vec<Rc<ModuleSpec<B>>>,
    natives: Vec<Rc<NativeModule<B>>>,
    bundles: Vec<Bundle>,
    denied: HashSet<String>,
    host_initializers: Vec<HostInitializer<B>>,
    native_initializers: Vec<NativeInitializer<B>>,
    config: B::Config,
    policy: ExecutionPolicy,
}

impl<B: Backend> RuntimeBuilder<B> {
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

    pub fn config(mut self, config: B::Config) -> Self {
        self.config = config;

        self
    }

    pub fn timeout(mut self, budget: Duration) -> Self {
        self.policy = self.policy.timeout(budget);

        self
    }

    pub fn cancellation<S: CancelSignal>(mut self, signal: S) -> Self {
        self.policy = self.policy.cancellation(signal);

        self
    }

    pub fn cancel_poll_interval(mut self, interval: Duration) -> Self {
        self.policy = self
            .policy
            .cancel_poll_interval(interval);

        self
    }
}

impl<B> RuntimeBuilder<B>
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

impl<B> RuntimeBuilder<B>
where
    B: Backend + BackendValues + BackendCallables + BackendModules,
{
    pub fn build(self) -> Result<Runtime<B>, Error> {
        let engine = B::engine(self.config)?;
        let real_import =
            B::enter(&engine, |token| Ok::<_, Error>(B::detach(token, B::real_import(token)?)))?;
        let catalog = Catalog::new(
            self.modules,
            self.natives,
            self.bundles,
            self.denied,
            self.host_initializers,
            self.native_initializers,
        );
        let realisation = RealisationCache::new();

        for module in catalog.modules() {
            realisation.absorb(module);
        }

        let inner = Rc::new(RuntimeInner {
            engine,
            catalog,
            realisation,
            registry: GuestRegistry::new(),
            next_id: Cell::new(1),
            policy: self.policy,
            real_import,
            release: Cell::new(false),
        });

        B::enter(&inner.engine, |token| {
            B::install_dispatcher(
                token,
                &inner.engine,
                B::function(token, "__import__", None, GuestInner::import_body(&inner))?,
            )
        })?;

        Ok(Runtime { inner })
    }
}

#[cfg(test)]
mod tests {
    use super::Runtime;
    use crate::{backend::tests::Stub, bundle::Bundle, host::module::ModuleSpec};

    #[allow(dead_code)]
    fn builder_chain() {
        let runtime = Runtime::<Stub>::builder()
            .bind(ModuleSpec::new("runtime"))
            .bundle(Bundle::single("library", "").unwrap())
            .deny("subprocess")
            .build()
            .unwrap();

        let guest = runtime
            .guest()
            .bind(ModuleSpec::new("guest"))
            .build()
            .unwrap();

        drop(guest);
        drop(runtime);
    }
}
