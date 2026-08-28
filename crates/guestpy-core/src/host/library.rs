use std::rc::Rc;

use crate::{backend::Backend, errors::Error, host::module::ModuleSpec, scope::Enter};

pub(crate) type InitializeFn<B> = Rc<dyn for<'py> Fn(&Enter<'py, B>) -> Result<(), Error>>;

pub(crate) enum HostLibraryEntry<B: Backend> {
    Module(Rc<ModuleSpec<B>>),
    Initializer(HostInitializer<B>),
}

pub struct HostInitializer<B: Backend> {
    initialize: InitializeFn<B>,
}

impl<B: Backend> HostInitializer<B> {
    pub fn new<F>(initialize: F) -> Self
    where
        F: for<'py> Fn(&Enter<'py, B>) -> Result<(), Error> + 'static,
    {
        Self { initialize: Rc::new(initialize) }
    }

    pub(crate) fn run<'py>(&self, enter: &Enter<'py, B>) -> Result<(), Error> {
        (self.initialize)(enter)
    }
}

pub struct HostLibrary<B: Backend> {
    entries: Vec<HostLibraryEntry<B>>,
}

impl<B: Backend> HostLibrary<B> {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    pub(crate) fn into_entries(self) -> Vec<HostLibraryEntry<B>> {
        self.entries
    }

    pub fn with(mut self, module: ModuleSpec<B>) -> Self {
        self.entries
            .push(HostLibraryEntry::Module(Rc::new(module)));

        self
    }

    pub fn initialize(mut self, initializer: HostInitializer<B>) -> Self {
        self.entries
            .push(HostLibraryEntry::Initializer(initializer));

        self
    }

    pub fn extend(mut self, library: impl Into<HostLibrary<B>>) -> Self {
        self.entries
            .extend(library.into().entries);

        self
    }
}

impl<B: Backend> Default for HostLibrary<B> {
    fn default() -> Self {
        Self::new()
    }
}

impl<B: Backend> From<ModuleSpec<B>> for HostLibrary<B> {
    fn from(module: ModuleSpec<B>) -> Self {
        Self::new().with(module)
    }
}

#[cfg(test)]
mod tests {
    use super::{HostInitializer, HostLibrary, HostLibraryEntry};
    use crate::{backend::tests::Stub, errors::Error, host::module::ModuleSpec};

    #[test]
    fn converts_a_module_spec_into_a_library() {
        match HostLibrary::<Stub>::from(ModuleSpec::new("first"))
            .into_entries()
            .remove(0)
        {
            HostLibraryEntry::Module(module) => assert_eq!(module.name(), "first"),
            HostLibraryEntry::Initializer(_) => panic!("expected a host module"),
        }
    }

    #[test]
    fn preserves_heterogeneous_entry_order() {
        let entries = HostLibrary::<Stub>::new()
            .with(ModuleSpec::new("first"))
            .initialize(HostInitializer::new(|_| Ok::<_, Error>(())))
            .with(ModuleSpec::new("second"))
            .into_entries();

        assert!(matches!(
            &entries[0],
            HostLibraryEntry::Module(module) if module.name() == "first"
        ));
        assert!(matches!(&entries[1], HostLibraryEntry::Initializer(_)));
        assert!(matches!(
            &entries[2],
            HostLibraryEntry::Module(module) if module.name() == "second"
        ));
    }
}
