use std::rc::Rc;

use crate::{
    backend::{Backend, BackendLibrary, Tok, Val},
    errors::Error,
    scope::Enter,
};

pub(crate) type InitializeNativeFn<B> = Rc<dyn for<'py> Fn(&Enter<'py, B>) -> Result<(), Error>>;

pub(crate) trait DeclareNative<B: Backend> {
    fn declare<'py>(&self, token: Tok<'py, B>, name: &str) -> Result<Val<'py, B>, Error>;
}

struct NativeDeclaration<B: BackendLibrary> {
    native: B::NativeModule,
}

impl<B: BackendLibrary> DeclareNative<B> for NativeDeclaration<B> {
    fn declare<'py>(&self, token: Tok<'py, B>, name: &str) -> Result<Val<'py, B>, Error> {
        B::declare_native(token, &self.native, name)
    }
}

pub(crate) enum NativeLibraryEntry<B: Backend> {
    Module(NativeModule<B>),
    Initializer(NativeInitializer<B>),
}

pub struct NativeInitializer<B: Backend> {
    initialize: InitializeNativeFn<B>,
}

impl<B: Backend> NativeInitializer<B> {
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

pub struct NativeModule<B: Backend> {
    name: String,
    aliases: Vec<String>,
    declare: Rc<dyn DeclareNative<B>>,
}

impl<B: Backend> NativeModule<B> {
    pub fn new<N>(name: N, native: B::NativeModule) -> Self
    where
        N: Into<String>,
        B: BackendLibrary,
    {
        Self {
            name: name.into(),
            aliases: Vec::new(),
            declare: Rc::new(NativeDeclaration { native }),
        }
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn aliases(&self) -> &[String] {
        &self.aliases
    }

    pub(crate) fn declare<'py>(
        &self,
        token: Tok<'py, B>,
        name: &str,
    ) -> Result<Val<'py, B>, Error> {
        self.declare.declare(token, name)
    }

    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        let alias = alias.into();

        if alias != self.name && !self.aliases.contains(&alias) {
            self.aliases.push(alias);
        }

        self
    }
}

pub struct NativeLibrary<B: Backend> {
    entries: Vec<NativeLibraryEntry<B>>,
}

impl<B: Backend> NativeLibrary<B> {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    pub(crate) fn into_entries(self) -> Vec<NativeLibraryEntry<B>> {
        self.entries
    }

    pub fn with(mut self, module: NativeModule<B>) -> Self {
        self.entries
            .push(NativeLibraryEntry::Module(module));

        self
    }

    pub fn initialize(mut self, initializer: NativeInitializer<B>) -> Self {
        self.entries
            .push(NativeLibraryEntry::Initializer(initializer));

        self
    }

    pub fn extend(mut self, library: impl Into<NativeLibrary<B>>) -> Self {
        self.entries
            .extend(library.into().entries);

        self
    }
}

impl<B: Backend> Default for NativeLibrary<B> {
    fn default() -> Self {
        Self::new()
    }
}

impl<B: Backend> From<NativeModule<B>> for NativeLibrary<B> {
    fn from(module: NativeModule<B>) -> Self {
        Self::new().with(module)
    }
}

#[cfg(test)]
mod tests {
    use super::{NativeInitializer, NativeLibrary, NativeLibraryEntry, NativeModule};
    use crate::{
        backend::tests::{Stub, StubValue},
        errors::Error,
    };

    #[test]
    fn converts_a_native_module_into_a_library() {
        match NativeLibrary::<Stub>::from(NativeModule::<Stub>::new("first", StubValue::None))
            .into_entries()
            .remove(0)
        {
            NativeLibraryEntry::Module(module) => assert_eq!(module.name(), "first"),
            NativeLibraryEntry::Initializer(_) => panic!("expected a native module"),
        }
    }

    #[test]
    fn deduplicates_and_orders_aliases() {
        let module = NativeModule::<Stub>::new("module", StubValue::None)
            .alias("module")
            .alias("mod:alias")
            .alias("mod:alias")
            .alias("pkg:module");

        assert_eq!(module.name(), "module");
        assert_eq!(module.aliases(), ["mod:alias", "pkg:module"]);
    }

    #[test]
    fn extends_in_entry_order() {
        let entries = NativeLibrary::<Stub>::new()
            .with(NativeModule::<Stub>::new("first", StubValue::None))
            .initialize(NativeInitializer::new(|_| Ok::<_, Error>(())))
            .extend(
                NativeLibrary::new()
                    .with(NativeModule::<Stub>::new("second", StubValue::None))
                    .initialize(NativeInitializer::new(|_| Ok::<_, Error>(()))),
            )
            .into_entries();

        assert!(matches!(
            &entries[0],
            NativeLibraryEntry::Module(module) if module.name() == "first"
        ));
        assert!(matches!(&entries[1], NativeLibraryEntry::Initializer(_)));
        assert!(matches!(
            &entries[2],
            NativeLibraryEntry::Module(module) if module.name() == "second"
        ));
        assert!(matches!(&entries[3], NativeLibraryEntry::Initializer(_)));
    }
}
