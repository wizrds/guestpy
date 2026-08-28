//! Virtual Python source bundles.

use std::{
    collections::BTreeMap,
    fmt::{self, Debug, Formatter},
    num::NonZeroU64,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

#[cfg(feature = "tokio")]
use std::path::Path;

#[cfg(feature = "embedded")]
use crate::embed::{Dir, DirEntry};
use crate::errors::Error;

#[cfg(feature = "tokio")]
use tokio::fs;

pub(crate) struct BundleModule {
    source: Arc<str>,
    origin: String,
    package: bool,
}

impl BundleModule {
    pub(crate) fn source(&self) -> &str {
        &self.source
    }

    pub(crate) fn origin(&self) -> &str {
        &self.origin
    }

    pub(crate) fn is_package(&self) -> bool {
        self.package
    }
}

struct BundleInner {
    id: BundleId,
    root: Option<String>,
    modules: BTreeMap<String, BundleModule>,
    data: BTreeMap<String, Arc<[u8]>>,
}

static NEXT_BUNDLE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct BundleId(NonZeroU64);

impl BundleId {
    fn next() -> Self {
        Self(
            NonZeroU64::new(
                NEXT_BUNDLE_ID.fetch_add(1, Ordering::Relaxed),
            )
            .expect("bundle IDs begin at one"),
        )
    }
}

#[derive(Clone)]
pub struct Bundle(Arc<BundleInner>);

impl Bundle {
    pub fn single(name: &str, source: impl Into<Arc<str>>) -> Result<Self, Error> {
        Self::builder()
            .module(name, source)
            .build()
    }

    #[cfg(feature = "tokio")]
    pub async fn from_dir(path: impl AsRef<Path>) -> Result<Self, Error> {
        BundleBuilder::from_dir(path)
            .await?
            .build()
    }

    #[cfg(feature = "embedded")]
    pub fn from_embedded(dir: &Dir<'_>) -> Result<Self, Error> {
        BundleBuilder::from_embedded(dir)?.build()
    }

    pub fn builder() -> BundleBuilder {
        BundleBuilder::default()
    }

    pub fn id(&self) -> BundleId {
        self.0.id
    }

    pub fn root(&self) -> Option<&str> {
        self.0.root.as_deref()
    }

    pub(crate) fn roots(&self) -> usize {
        self.0
            .modules
            .keys()
            .filter(|name| !name.contains('.'))
            .count()
    }

    pub fn contains(&self, dotted: &str) -> bool {
        self.0.modules.contains_key(dotted)
    }

    pub fn data(&self, path: &str) -> Option<&Arc<[u8]>> {
        self.0.data.get(path)
    }

    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.0
            .modules
            .keys()
            .map(String::as_str)
    }

    pub(crate) fn module(&self, dotted: &str) -> Option<&BundleModule> {
        self.0.modules.get(dotted)
    }
}

impl Debug for Bundle {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Bundle")
            .field("id", &self.id())
            .field("root", &self.root())
            .field("modules", &self.0.modules.len())
            .finish()
    }
}

#[derive(Default)]
pub struct BundleBuilder {
    modules: BTreeMap<String, BundleModule>,
    data: BTreeMap<String, Arc<[u8]>>,
    error: Option<Error>,
}

impl BundleBuilder {
    #[cfg(feature = "tokio")]
    async fn from_dir(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        let root = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                Error::bundle(
                    path.display().to_string(),
                    "path has no UTF-8 name",
                )
            })?;

        let is_package = fs::try_exists(path.join("__init__.py"))
            .await
            .unwrap_or(false);
        
        let prefix = is_package.then(|| root.to_owned()).unwrap_or_default();

        let mut builder = Self::default();
        let mut directories = vec![(path.to_owned(), prefix)];

        while let Some((directory, relative)) = directories.pop() {
            let mut entries = fs::read_dir(&directory).await?;
            let mut children = Vec::new();

            while let Some(entry) = entries.next_entry().await? {
                children.push(entry);
            }

            children.sort_by_key(|entry| entry.file_name());

            for entry in children.into_iter().rev() {
                let name = entry
                    .file_name()
                    .to_string_lossy()
                    .into_owned();

                let relative = if relative.is_empty() {
                    name
                } else {
                    format!("{relative}/{name}")
                };

                if entry.file_type().await?.is_dir() {
                    directories.push((entry.path(), relative));
                } else {
                    builder.insert_entry(
                        &relative,
                        &fs::read(entry.path()).await?,
                    )?;
                }
            }
        }

        Ok(builder)
    }

    #[cfg(feature = "embedded")]
    fn from_embedded(dir: &Dir<'_>) -> Result<Self, Error> {
        let root = dir
            .path()
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                Error::bundle(
                    "<embedded>",
                    "path has no UTF-8 name",
                )
            })?;
        let is_package = dir
            .entries()
            .iter()
            .any(|entry| matches!(
                entry,
                DirEntry::File(file)
                    if file.path().file_name().and_then(|name| name.to_str()) == Some("__init__.py")
            ));

        let mut builder = Self::default();
        builder.insert_embedded_dir(is_package.then_some(root), dir)?;

        Ok(builder)
    }

    fn is_identifier(name: &str) -> bool {
        let mut characters = name.chars();

        matches!(characters.next(), Some(character) if character == '_' || character.is_ascii_alphabetic())
            && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
    }

    fn validate_dotted(dotted: &str) -> Result<(), Error> {
        if dotted.is_empty() {
            return Err(Error::bundle(dotted, "module name is empty"));
        }

        for component in dotted.split('.') {
            if !Self::is_identifier(component) {
                return Err(Error::bundle(
                    dotted,
                    format!("'{component}' is not a valid Python module name"),
                ));
            }
        }

        Ok(())
    }

    fn origin(dotted: &str, package: bool) -> String {
        let path = dotted.replace('.', "/");

        if package {
            format!("{path}/__init__.py")
        } else {
            format!("{path}.py")
        }
    }

    fn insert_module(&mut self, dotted: &str, source: Arc<str>, package: bool) {
        if self.error.is_some() {
            return;
        }

        if let Err(error) = Self::validate_dotted(dotted) {
            self.error = Some(error);

            return;
        }

        self.modules.insert(
            dotted.to_owned(),
            BundleModule {
                source,
                origin: Self::origin(dotted, package),
                package,
            },
        );
    }

    #[cfg(any(feature = "tokio", feature = "embedded"))]
    fn insert_entry(&mut self, relative: &str, contents: &[u8]) -> Result<(), Error> {
        let normalized = relative.replace('\\', "/");

        if normalized.ends_with(".so")
            || normalized.ends_with(".pyd")
            || normalized.ends_with(".dylib")
        {
            return Err(Error::bundle(
                normalized,
                "compiled extension modules are not supported; a bundle is pure Python",
            ));
        }

        if !normalized.ends_with(".py") {
            self.data
                .insert(normalized, Arc::from(contents));

            return Ok(());
        }

        let mut parts = normalized
            .split('/')
            .collect::<Vec<_>>();
        let filename = parts.pop().unwrap();
        let stem = filename.strip_suffix(".py").unwrap();

        for component in parts
            .iter()
            .copied()
            .chain(std::iter::once(stem))
        {
            if !Self::is_identifier(component) {
                return Err(Error::bundle(
                    &normalized,
                    format!("'{component}' is not a valid Python module name"),
                ));
            }
        }

        let package = stem == "__init__";
        let dotted = if package {
            parts.join(".")
        } else {
            parts
                .into_iter()
                .chain(std::iter::once(stem))
                .collect::<Vec<_>>()
                .join(".")
        };
        let source = std::str::from_utf8(contents)
            .map_err(|_| Error::bundle(normalized.clone(), "source is not valid UTF-8"))?;

        self.insert_module(&dotted, Arc::from(source), package);

        Ok(())
    }

    #[cfg(feature = "embedded")]
    fn insert_embedded_dir(&mut self, prefix: Option<&str>, dir: &Dir<'_>) -> Result<(), Error> {
        for entry in dir.entries() {
            match entry {
                DirEntry::Dir(directory) => {
                    self.insert_embedded_dir(prefix, directory)?;
                }
                DirEntry::File(file) => {
                    let relative = file.path().to_string_lossy();
                    let normalized = match prefix {
                        Some(prefix) => format!("{prefix}/{relative}"),
                        None => relative.into_owned(),
                    };

                    self.insert_entry(&normalized, file.contents())?;
                }
            }
        }

        Ok(())
    }

    pub fn module(mut self, dotted: &str, source: impl Into<Arc<str>>) -> Self {
        self.insert_module(dotted, source.into(), false);

        self
    }

    pub fn package(mut self, dotted: &str, source: impl Into<Arc<str>>) -> Self {
        self.insert_module(dotted, source.into(), true);

        self
    }

    pub fn data(mut self, path: &str, bytes: impl Into<Arc<[u8]>>) -> Self {
        self.data
            .insert(path.to_owned(), bytes.into());

        self
    }

    pub fn build(self) -> Result<Bundle, Error> {
        if let Some(error) = self.error {
            return Err(error);
        }

        for dotted in self.modules.keys() {
            let mut parent = dotted.as_str();

            while let Some((prefix, _)) = parent.rsplit_once('.') {
                let Some(module) = self.modules.get(prefix) else {
                    return Err(Error::bundle(dotted, format!("{dotted} has no parent package")));
                };

                if !module.package {
                    return Err(Error::bundle(dotted, format!("{dotted} has no parent package")));
                }

                parent = prefix;
            }
        }

        let roots = self
            .modules
            .keys()
            .filter(|name| !name.contains('.'))
            .collect::<Vec<_>>();
        let root = (roots.len() == 1).then(|| roots[0].to_owned());

        Ok(Bundle(Arc::new(BundleInner {
            id: BundleId::next(),
            root,
            modules: self.modules,
            data: self.data,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::Bundle;

    #[cfg(feature = "tokio")]
    use std::{
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    #[cfg(feature = "embedded")]
    use crate::embed::{Dir, DirEntry, File};

    #[cfg(feature = "tokio")]
    static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(1);

    #[cfg(feature = "tokio")]
    struct DirectoryFixture {
        base: PathBuf,
        root: PathBuf,
    }

    #[cfg(feature = "tokio")]
    impl DirectoryFixture {
        fn new() -> Self {
            let base = std::env::temp_dir().join(format!(
                "guestpy-bundle-{}-{}",
                std::process::id(),
                NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed),
            ));
            let root = base.join("plugin");

            std::fs::create_dir_all(&root).unwrap();

            Self { base, root }
        }

        fn root(&self) -> &Path {
            &self.root
        }

        fn write(&self, relative: &str, contents: &[u8]) {
            let path = self.root.join(relative);

            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }

            std::fs::write(path, contents).unwrap();
        }
    }

    #[cfg(feature = "tokio")]
    impl Drop for DirectoryFixture {
        fn drop(&mut self) {
            if let Err(error) = std::fs::remove_dir_all(&self.base) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    panic!("failed to remove bundle fixture: {error}");
                }
            }
        }
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn from_dir_builds_the_same_bundle_shape() {
        let fixture = DirectoryFixture::new();

        fixture.write("__init__.py", b"VALUE = 1");
        fixture.write("util.py", b"VALUE = 2");
        fixture.write("data.json", br#"{"enabled":true}"#);

        let bundle = Bundle::from_dir(fixture.root())
            .await
            .unwrap();

        assert_eq!(bundle.root(), Some("plugin"));
        assert!(bundle.contains("plugin"));
        assert!(bundle.contains("plugin.util"));
        assert_eq!(
            bundle
                .module("plugin")
                .unwrap()
                .origin(),
            "plugin/__init__.py",
        );
        assert_eq!(
            bundle
                .module("plugin.util")
                .unwrap()
                .origin(),
            "plugin/util.py",
        );
        assert_eq!(
            bundle
                .data("plugin/data.json")
                .unwrap()
                .as_ref(),
            br#"{"enabled":true}"#,
        );
    }

    #[cfg(feature = "tokio")]
    #[tokio::test]
    async fn from_dir_rejects_compiled_extensions() {
        let fixture = DirectoryFixture::new();

        fixture.write("extension.so", b"");

        assert!(
            Bundle::from_dir(fixture.root())
                .await
                .unwrap_err()
                .to_string()
                .contains("compiled extension modules are not supported"),
        );
    }

    #[cfg(feature = "embedded")]
    #[test]
    fn embedded_builder_preserves_entry_rules() {
        const ENTRIES: &[DirEntry<'static>] = &[
            DirEntry::File(File::new("__init__.py", b"VALUE = 1")),
            DirEntry::File(File::new("util.py", b"VALUE = 2")),
            DirEntry::File(File::new("data.json", br#"{"enabled":true}"#)),
        ];
        const ROOT: Dir<'static> = Dir::new("plugin", ENTRIES);

        let bundle = Bundle::from_embedded(&ROOT).unwrap();

        assert_eq!(bundle.root(), Some("plugin"));
        assert!(bundle.contains("plugin"));
        assert!(bundle.contains("plugin.util"));
        assert_eq!(
            bundle
                .data("plugin/data.json")
                .unwrap()
                .as_ref(),
            br#"{"enabled":true}"#,
        );
    }

    #[test]
    fn rejects_orphan_submodule() {
        let error = Bundle::builder()
            .module("plugin.util", "X = 1")
            .build()
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("has no parent package")
        );
    }

    #[test]
    fn builder_keeps_first_error() {
        let error = Bundle::builder()
            .module("my-plugin", "X = 1")
            .module("plugin", "X = 1")
            .build()
            .unwrap_err();

        assert!(error.to_string().contains("my-plugin"));
    }

    #[test]
    fn ids_are_distinct_and_clone_preserves() {
        let first = Bundle::single("plugin", "X = 1").unwrap();
        let second = Bundle::single("plugin", "X = 1").unwrap();

        assert_ne!(first.id(), second.id());
        assert_eq!(first.id(), first.clone().id());
    }

    #[test]
    fn single_validates_its_name() {
        assert!(Bundle::single("my-plugin", "X = 1").is_err());
    }

    #[test]
    fn origins_are_source_relative() {
        let bundle = Bundle::builder()
            .package("plugin", "")
            .package("plugin.handlers", "")
            .module("plugin.handlers.http", "")
            .build()
            .unwrap();

        assert_eq!(
            bundle
                .module("plugin")
                .unwrap()
                .origin(),
            "plugin/__init__.py",
        );
        assert_eq!(
            bundle
                .module("plugin.handlers.http")
                .unwrap()
                .origin(),
            "plugin/handlers/http.py",
        );
        assert!(
            bundle
                .module("plugin")
                .unwrap()
                .is_package()
        );
        assert!(
            !bundle
                .module("plugin.handlers.http")
                .unwrap()
                .is_package(),
        );
    }

    #[test]
    fn a_single_root_is_named_and_counted() {
        let bundle = Bundle::builder()
            .package("plugin", "")
            .module("plugin.util", "")
            .build()
            .unwrap();

        assert_eq!(bundle.root(), Some("plugin"));
        assert_eq!(bundle.roots(), 1);
    }

    #[test]
    fn multi_root_has_no_root() {
        let bundle = Bundle::builder()
            .package("alpha", "")
            .package("beta", "")
            .module("beta.deep", "")
            .build()
            .unwrap();

        assert_eq!(bundle.root(), None);
        assert_eq!(bundle.roots(), 2);
    }

    #[test]
    fn carries_data_files() {
        let bundle = Bundle::builder()
            .package("plugin", "")
            .data("plugin/data/config.json", b"{\"enabled\": true}".to_vec())
            .build()
            .unwrap();

        assert_eq!(
            bundle
                .data("plugin/data/config.json")
                .unwrap()
                .as_ref(),
            b"{\"enabled\": true}",
        );
        assert!(
            bundle
                .data("plugin/data/missing.json")
                .is_none()
        );
    }

    #[test]
    fn rejects_a_non_package_parent() {
        assert!(
            Bundle::builder()
                .module("pkg", "")
                .module("pkg.mod", "")
                .build()
                .unwrap_err()
                .to_string()
                .contains("has no parent package"),
        );
    }

    #[test]
    fn rejects_a_non_identifier_component() {
        assert!(
            Bundle::builder()
                .module("my-package.mod", "")
                .build()
                .unwrap_err()
                .to_string()
                .contains("my-package"),
        );
    }
}
