use std::{
    collections::HashMap,
    fs,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

use guestpy_core::{
    backend::{NativeExtensionContext, NativeExtensionLoader, PreparedNativeExtensions, Tok, Val},
    bundle::{Bundle, BundleId},
    errors::Error,
};
use pyo3::{
    Bound, Python,
    types::{PyAny, PyAnyMethods, PyDict},
};
use tempfile::{TempDir, tempdir};

use crate::{
    engine::{CPython, Object},
    errors::NativeErrors,
};

static NATIVE_EXTENSION_STORE: OnceLock<CPythonNativeExtensionStore> = OnceLock::new();
static NATIVE_EXTENSION_REGISTRY: OnceLock<Mutex<CPythonNativeExtensionRegistry>> = OnceLock::new();

struct NativeArtifact {
    path: PathBuf,
    contents: Arc<[u8]>,
}

struct MaterializedBundle {
    root: PathBuf,
    modules: HashMap<String, NativeArtifact>,
    incompatible: HashMap<String, Vec<String>>,
}

#[derive(Clone)]
struct LoadedExtension {
    origin: PathBuf,
    contents: Arc<[u8]>,
    module: Object,
}

struct CPythonNativeExtensionStore {
    root: TempDir,
    import_lock: Object,
}

impl CPythonNativeExtensionStore {
    fn new(py: Python<'_>) -> Result<Self, Error> {
        let root = tempdir()?;
        let lock = py
            .import("threading")
            .and_then(|threading| threading.getattr("RLock"))
            .and_then(|rlock| rlock.call0())
            .map_err(|error| CPython::guest(py, error))?;

        Ok(Self {
            root,
            import_lock: Object::new(lock.unbind()),
        })
    }
}

#[derive(Default)]
struct CPythonNativeExtensionRegistry {
    bundles: HashMap<BundleId, Arc<MaterializedBundle>>,
    loaded: HashMap<String, LoadedExtension>,
}

struct PythonImportLock<'py> {
    lock: Bound<'py, PyAny>,
}

impl<'py> PythonImportLock<'py> {
    fn acquire(py: Python<'py>, lock: &Object) -> Result<Self, Error> {
        let lock = lock.bind(py);

        lock.call_method0("acquire")
            .map_err(|error| CPython::guest(py, error))?;

        Ok(Self { lock })
    }
}

impl Drop for PythonImportLock<'_> {
    fn drop(&mut self) {
        // A successfully acquired `threading.RLock` must still be released while unwinding;
        // `Drop` cannot propagate the failure of that release call.
        let _ = self.lock.call_method0("release");
    }
}

pub struct PreparedCPythonExtensions {
    bundle: Arc<MaterializedBundle>,
}

pub struct CPythonNativeExtensions;

impl CPythonNativeExtensions {
    fn store(py: Python<'_>) -> Result<&'static CPythonNativeExtensionStore, Error> {
        if let Some(store) = NATIVE_EXTENSION_STORE.get() {
            return Ok(store);
        }

        let store = CPythonNativeExtensionStore::new(py)?;

        Ok(NATIVE_EXTENSION_STORE.get_or_init(|| store))
    }

    fn with_registry<R>(
        operation: impl FnOnce(&mut CPythonNativeExtensionRegistry) -> Result<R, Error>,
    ) -> Result<R, Error> {
        let mut guard = NATIVE_EXTENSION_REGISTRY
            .get_or_init(|| Mutex::new(CPythonNativeExtensionRegistry::default()))
            .lock()
            .map_err(|_| Error::unexpected("the CPython native-extension registry is poisoned"))?;

        operation(&mut guard)
    }

    fn import_lock(py: Python<'_>) -> Result<Object, Error> {
        Ok(Self::store(py)?.import_lock.clone())
    }

    fn loaded(dotted: &str) -> Result<Option<LoadedExtension>, Error> {
        Self::with_registry(|registry| Ok(registry.loaded.get(dotted).cloned()))
    }

    fn record_loaded(dotted: &str, loaded: LoadedExtension) -> Result<(), Error> {
        Self::with_registry(|registry| {
            registry
                .loaded
                .insert(dotted.to_owned(), loaded);

            Ok(())
        })
    }

    fn extension_suffixes(py: Python<'_>) -> Result<Vec<String>, Error> {
        let mut suffixes = py
            .import("importlib.machinery")
            .and_then(|module| module.getattr("EXTENSION_SUFFIXES"))
            .and_then(|suffixes| suffixes.extract::<Vec<String>>())
            .map_err(|error| CPython::guest(py, error))?;

        suffixes.sort_by_key(|suffix| std::cmp::Reverse(suffix.len()));
        suffixes.dedup();

        Ok(suffixes)
    }

    fn is_identifier(name: &str) -> bool {
        let mut characters = name.chars();

        matches!(characters.next(), Some(character) if character == '_' || character.is_ascii_alphabetic())
            && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
    }

    fn native_ending(path: &str) -> bool {
        path.ends_with(".so") || path.ends_with(".pyd") || path.ends_with(".dylib")
    }

    fn compatible_name(path: &str, suffixes: &[String]) -> Option<String> {
        let suffix = suffixes
            .iter()
            .find(|suffix| path.ends_with(suffix.as_str()))?;
        let stripped = &path[..path.len() - suffix.len()];
        let mut parts = stripped.split('/').collect::<Vec<_>>();
        let basename = parts.pop()?;

        if !parts
            .iter()
            .copied()
            .chain(std::iter::once(basename))
            .all(Self::is_identifier)
        {
            return None;
        }

        parts.push(basename);

        Some(parts.join("."))
    }

    fn incompatible_name(path: &str) -> Option<String> {
        if !Self::native_ending(path) {
            return None;
        }

        let stripped = path
            .strip_suffix(".so")
            .or_else(|| path.strip_suffix(".pyd"))
            .or_else(|| path.strip_suffix(".dylib"))?;
        let mut parts = stripped.split('/').collect::<Vec<_>>();
        let filename = parts.pop()?;
        let basename = filename
            .split_once('.')
            .map_or(filename, |(basename, _)| basename);

        if !parts
            .iter()
            .copied()
            .chain(std::iter::once(basename))
            .all(Self::is_identifier)
        {
            return None;
        }

        parts.push(basename);

        Some(parts.join("."))
    }

    fn validate_relative(path: &str) -> Result<PathBuf, Error> {
        let mut relative = PathBuf::new();

        for component in Path::new(path).components() {
            match component {
                Component::Normal(part) => relative.push(part),
                Component::CurDir => {}
                _ => {
                    return Err(Error::unexpected(format!(
                        "bundle path '{path}' escapes its materialized root",
                    )));
                }
            }
        }

        Ok(relative)
    }

    fn write_bundle(
        root: &Path,
        bundle: &Bundle,
        suffixes: &[String],
    ) -> Result<MaterializedBundle, Error> {
        let mut modules = HashMap::new();
        let mut incompatible: HashMap<String, Vec<String>> = HashMap::new();

        for (relative, contents) in bundle.files() {
            let path = root.join(Self::validate_relative(relative)?);

            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }

            fs::write(&path, contents)?;

            if let Some(name) = Self::compatible_name(relative, suffixes) {
                modules.insert(name, NativeArtifact { path, contents: Arc::from(contents) });
            } else if let Some(name) = Self::incompatible_name(relative) {
                incompatible
                    .entry(name)
                    .or_default()
                    .push(relative.to_owned());
            }
        }

        Ok(MaterializedBundle {
            root: root.to_owned(),
            modules,
            incompatible,
        })
    }

    fn materialize(
        root: &Path,
        registry: &mut CPythonNativeExtensionRegistry,
        bundle: &Bundle,
        suffixes: &[String],
    ) -> Result<Arc<MaterializedBundle>, Error> {
        if let Some(materialized) = registry.bundles.get(&bundle.id()) {
            return Ok(materialized.clone());
        }

        let root = root.join(format!("bundle-{}", bundle.id().value()));

        match Self::write_bundle(&root, bundle, suffixes) {
            Ok(materialized) => {
                let materialized = Arc::new(materialized);

                registry
                    .bundles
                    .insert(bundle.id(), materialized.clone());

                Ok(materialized)
            }
            Err(error) => {
                let _ = fs::remove_dir_all(&root);

                Err(error)
            }
        }
    }
}

impl PreparedCPythonExtensions {
    fn sys_modules<'py>(py: Python<'py>) -> Result<Bound<'py, PyAny>, Error> {
        py.import("sys")
            .and_then(|sys| sys.getattr("modules"))
            .map_err(|error| CPython::guest(py, error))
    }

    fn remove_if_same(
        modules: &Bound<'_, PyAny>,
        name: &str,
        expected: &Bound<'_, PyAny>,
    ) -> Result<(), Error> {
        match modules.get_item(name) {
            Ok(current) if current.is(expected) => modules
                .del_item(name)
                .map_err(|error| CPython::guest(modules.py(), error)),
            _ => Ok(()),
        }
    }

    fn rollback<'py>(
        modules: &Bound<'py, PyAny>,
        inserted_parents: Vec<(String, Bound<'py, PyAny>)>,
        child: Option<(&str, &Bound<'py, PyAny>)>,
    ) -> Result<(), Error> {
        if let Some((dotted, module)) = child {
            Self::remove_if_same(modules, dotted, module)?;
        }

        for (name, parent) in inserted_parents.into_iter().rev() {
            Self::remove_if_same(modules, &name, &parent)?;
        }

        Ok(())
    }

    fn register_parents<'py>(
        context: &NativeExtensionContext<'py, CPython>,
        modules: &Bound<'py, PyAny>,
    ) -> Result<Vec<(String, Bound<'py, PyAny>)>, Error> {
        let py = modules.py();
        let mut inserted = Vec::new();

        for (name, parent) in context.parents() {
            match modules.get_item(name) {
                Ok(existing) if existing.is(parent) => {}
                Ok(_) => {
                    Self::rollback(modules, inserted, None)?;

                    return Err(Error::import(
                        name,
                        format!("a different parent module already occupies {name}"),
                    ));
                }
                Err(_) => {
                    if let Err(error) = modules.set_item(name, parent) {
                        Self::rollback(modules, inserted, None)?;

                        return Err(CPython::guest(py, error));
                    }

                    inserted.push((name.to_owned(), parent.clone()));
                }
            }
        }

        Ok(inserted)
    }

    fn extension_loader<'py>(
        py: Python<'py>,
        dotted: &str,
        path: &Path,
    ) -> Result<Bound<'py, PyAny>, Error> {
        py.import("importlib.machinery")
            .and_then(|machinery| machinery.getattr("ExtensionFileLoader"))
            .and_then(|class| class.call1((dotted, path.to_string_lossy().as_ref())))
            .map_err(|error| CPython::guest(py, error))
    }

    fn execute_loader<'py>(
        py: Python<'py>,
        context: &NativeExtensionContext<'py, CPython>,
        dotted: &str,
        path: &Path,
        loader: &Bound<'py, PyAny>,
    ) -> Result<Bound<'py, PyAny>, Error> {
        let util = py
            .import("importlib.util")
            .map_err(|error| CPython::guest(py, error))?;
        let modules = Self::sys_modules(py)?;
        let kwargs = PyDict::new(py);

        kwargs
            .set_item("loader", loader)
            .map_err(|error| CPython::guest(py, error))?;

        let spec = util
            .call_method(
                "spec_from_file_location",
                (dotted, path.to_string_lossy().as_ref()),
                Some(&kwargs),
            )
            .map_err(|error| CPython::guest(py, error))?;

        if spec.is_none() {
            return Err(Error::import(dotted, "importlib did not create a module specification"));
        }

        let inserted = Self::register_parents(context, &modules)?;

        let module = match util.call_method1("module_from_spec", (&spec,)) {
            Ok(module) => module,
            Err(error) => {
                Self::rollback(&modules, inserted, None)?;

                return Err(CPython::guest(py, error));
            }
        };

        if let Err(error) = modules.set_item(dotted, &module) {
            Self::rollback(&modules, inserted, None)?;

            return Err(CPython::guest(py, error));
        }

        if let Err(error) = loader.call_method1("exec_module", (&module,)) {
            Self::rollback(&modules, inserted, Some((dotted, &module)))?;

            return Err(CPython::guest(py, error));
        }

        Ok(module)
    }

    fn load<'py>(
        &self,
        context: NativeExtensionContext<'py, CPython>,
        dotted: &str,
        artifact: &NativeArtifact,
    ) -> Result<Bound<'py, PyAny>, Error> {
        let py = context.token();
        let process_lock = CPythonNativeExtensions::import_lock(py)?;
        let _guard = PythonImportLock::acquire(py, &process_lock)?;

        if let Some(loaded) = CPythonNativeExtensions::loaded(dotted)? {
            if loaded.contents.as_ref() == artifact.contents.as_ref() {
                return Ok(loaded.module.bind(py));
            }

            return Err(Error::import(
                dotted,
                format!(
                    "a different native module is already loaded at {} for {dotted}; requested {}",
                    loaded.origin.display(),
                    artifact.path.display(),
                ),
            ));
        }

        let modules = Self::sys_modules(py)?;

        if modules.get_item(dotted).is_ok() {
            return Err(Error::import(
                dotted,
                format!("an ambient module already occupies {dotted}"),
            ));
        }

        let loader = Self::extension_loader(py, dotted, &artifact.path)?;
        let module = Self::execute_loader(py, &context, dotted, &artifact.path, &loader)?;

        CPythonNativeExtensions::record_loaded(
            dotted,
            LoadedExtension {
                origin: artifact.path.clone(),
                contents: artifact.contents.clone(),
                module: Object::new(module.clone().unbind()),
            },
        )?;

        Ok(module)
    }
}

impl NativeExtensionLoader<CPython> for CPythonNativeExtensions {
    type Prepared = PreparedCPythonExtensions;

    fn prepare<'py>(token: Tok<'py, CPython>, bundle: &Bundle) -> Result<Self::Prepared, Error> {
        let suffixes = Self::extension_suffixes(token)?;
        let store = Self::store(token)?;
        let bundle = Self::with_registry(|registry| {
            Self::materialize(store.root.path(), registry, bundle, &suffixes)
        })?;

        Ok(PreparedCPythonExtensions { bundle })
    }
}

impl PreparedNativeExtensions<CPython> for PreparedCPythonExtensions {
    fn names(&self) -> impl Iterator<Item = &str> {
        self.bundle
            .modules
            .keys()
            .map(String::as_str)
            .chain(
                self.bundle
                    .incompatible
                    .keys()
                    .filter(|name| !self.bundle.modules.contains_key(*name))
                    .map(String::as_str),
            )
    }

    fn realise<'py>(
        &self,
        context: NativeExtensionContext<'py, CPython>,
        dotted: &str,
    ) -> Result<Val<'py, CPython>, Error> {
        if let Some(artifact) = self.bundle.modules.get(dotted) {
            return self.load(context, dotted, artifact);
        }

        if let Some(candidates) = self.bundle.incompatible.get(dotted) {
            return Err(Error::import(
                dotted,
                format!(
                    "the bundle contains native artifacts for {dotted} ({}) but none match the \
                     active interpreter",
                    candidates.join(", "),
                ),
            ));
        }

        Err(Error::import(
            dotted,
            "the prepared bundle has no native extension of that name",
        ))
    }

    fn source_origin(&self, relative: &str) -> Option<String> {
        CPythonNativeExtensions::validate_relative(relative)
            .ok()
            .map(|validated| {
                self.bundle
                    .root
                    .join(validated)
                    .display()
                    .to_string()
            })
    }

    fn package_path(&self, dotted: &str) -> Option<String> {
        let path = self
            .bundle
            .root
            .join(dotted.replace('.', "/"));

        path.is_dir()
            .then(|| path.display().to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        ffi::CString,
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
    };

    use guestpy_core::{backend::NativeExtensionContext, bundle::Bundle};
    use pyo3::{
        Bound, PyAny, Python,
        types::{PyAnyMethods, PyDict, PyDictMethods},
    };

    use super::{
        CPythonNativeExtensions, MaterializedBundle, NativeArtifact, NativeExtensionLoader,
        PreparedCPythonExtensions, PreparedNativeExtensions,
    };
    use crate::engine::CPython;

    static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(1);

    fn unique_name(base: &str) -> String {
        format!(
            "{base}_{}_{}",
            std::process::id(),
            NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed),
        )
    }

    fn fixture_loaders(py: Python<'_>) -> (Bound<'_, PyAny>, Bound<'_, PyAny>) {
        let globals = PyDict::new(py);

        py.run(
            &CString::new(
                r#"
import types


class SuccessLoader:
    def create_module(self, spec):
        return types.ModuleType(spec.name)

    def exec_module(self, module):
        module.marker = True


class FailureLoader:
    def create_module(self, spec):
        return types.ModuleType(spec.name)

    def exec_module(self, module):
        raise RuntimeError('fixture failure')
"#,
            )
            .unwrap(),
            Some(&globals),
            None,
        )
        .unwrap();

        (
            globals
                .get_item("SuccessLoader")
                .unwrap()
                .unwrap()
                .call0()
                .unwrap(),
            globals
                .get_item("FailureLoader")
                .unwrap()
                .unwrap()
                .call0()
                .unwrap(),
        )
    }

    fn prepared_with(dotted: &str) -> PreparedCPythonExtensions {
        PreparedCPythonExtensions {
            bundle: Arc::new(MaterializedBundle {
                root: PathBuf::from("/tmp/guestpy-fixture-root"),
                modules: HashMap::from([(
                    dotted.to_owned(),
                    NativeArtifact {
                        path: PathBuf::from("/tmp/guestpy-fixture-root/fake.so"),
                        contents: Arc::from(b"fixture-bytes".as_slice()),
                    },
                )]),
                incompatible: HashMap::new(),
            }),
        }
    }

    fn mixed_bundle(native_suffix: &str) -> Bundle {
        Bundle::builder()
            .package("plugin", "")
            .data(&format!("plugin/_native{native_suffix}"), b"native-bytes".to_vec())
            .data("plugin/.libs/libdependency.so", b"dependency-bytes".to_vec())
            .data("plugin/legacy.so", b"legacy-bytes".to_vec())
            .build()
            .unwrap()
    }

    #[test]
    fn discovers_active_extension_suffixes() {
        Python::initialize();

        Python::attach(|py| {
            assert!(
                !CPythonNativeExtensions::extension_suffixes(py)
                    .unwrap()
                    .is_empty()
            );
        });
    }

    #[test]
    fn longest_suffix_derives_the_dotted_name() {
        assert_eq!(
            CPythonNativeExtensions::compatible_name(
                "plugin/_native.cpython-313-x86_64-linux-gnu.so",
                &[
                    ".cpython-313-x86_64-linux-gnu.so".to_owned(),
                    ".so".to_owned()
                ],
            ),
            Some("plugin._native".to_owned()),
        );
    }

    #[test]
    fn dependency_directories_do_not_become_claims() {
        assert_eq!(
            CPythonNativeExtensions::compatible_name(
                "plugin/.libs/libdependency.so",
                &[".so".to_owned()],
            ),
            None,
        );
        assert_eq!(
            CPythonNativeExtensions::incompatible_name("plugin/.libs/libdependency.so"),
            None
        );
    }

    #[test]
    fn incompatible_artifacts_are_recorded_by_logical_name() {
        assert_eq!(
            CPythonNativeExtensions::incompatible_name("plugin/legacy.so"),
            Some("plugin.legacy".to_owned()),
        );
    }

    #[test]
    fn materialization_preserves_the_complete_tree() {
        Python::initialize();

        Python::attach(|py| {
            let suffix = CPythonNativeExtensions::extension_suffixes(py)
                .unwrap()
                .into_iter()
                .next()
                .unwrap();
            let bundle = mixed_bundle(&suffix);
            let prepared =
                <CPythonNativeExtensions as NativeExtensionLoader<CPython>>::prepare(py, &bundle)
                    .unwrap();

            assert!(
                prepared
                    .bundle
                    .modules
                    .contains_key("plugin._native")
            );
            assert!(
                prepared
                    .bundle
                    .modules
                    .get("plugin._native")
                    .unwrap()
                    .path
                    .exists()
            );
            assert!(
                std::fs::read(
                    &prepared
                        .bundle
                        .root
                        .join("plugin/.libs/libdependency.so")
                )
                .unwrap()
                    == b"dependency-bytes"
            );
        });
    }

    #[test]
    fn materialization_rejects_absolute_and_parent_paths() {
        assert!(CPythonNativeExtensions::validate_relative("/etc/passwd").is_err());
        assert!(CPythonNativeExtensions::validate_relative("../escape.so").is_err());
        assert!(CPythonNativeExtensions::validate_relative("plugin/util.py").is_ok());
    }

    #[test]
    fn repeated_preparation_reuses_one_materialized_bundle() {
        Python::initialize();

        let bundle = mixed_bundle(".so");

        Python::attach(|py| {
            let first =
                <CPythonNativeExtensions as NativeExtensionLoader<CPython>>::prepare(py, &bundle)
                    .unwrap();
            let second =
                <CPythonNativeExtensions as NativeExtensionLoader<CPython>>::prepare(py, &bundle)
                    .unwrap();

            assert!(std::sync::Arc::ptr_eq(&first.bundle, &second.bundle));
        });
    }

    #[test]
    fn source_and_package_paths_point_into_the_materialized_tree() {
        Python::initialize();

        let bundle = mixed_bundle(".so");

        Python::attach(|py| {
            let prepared =
                <CPythonNativeExtensions as NativeExtensionLoader<CPython>>::prepare(py, &bundle)
                    .unwrap();
            let origin = prepared
                .source_origin("plugin/__init__.py")
                .unwrap();

            assert!(origin.ends_with("plugin/__init__.py"));
            assert!(std::path::Path::new(&origin).exists());

            let package = prepared.package_path("plugin").unwrap();

            assert!(std::path::Path::new(&package).is_dir());
            assert_eq!(prepared.package_path("plugin.missing"), None);
        });
    }

    #[test]
    fn import_lock_releases_after_success() {
        Python::initialize();

        Python::attach(|py| {
            let lock = py
                .import("threading")
                .unwrap()
                .getattr("Lock")
                .unwrap()
                .call0()
                .unwrap();
            let owned = crate::engine::Object::new(lock.clone().unbind());

            {
                let _guard = super::PythonImportLock::acquire(py, &owned).unwrap();

                assert!(
                    !lock
                        .call_method1("acquire", (false,))
                        .unwrap()
                        .extract::<bool>()
                        .unwrap()
                );
            }

            assert!(
                lock.call_method1("acquire", (false,))
                    .unwrap()
                    .extract::<bool>()
                    .unwrap()
            );

            lock.call_method0("release").unwrap();
        });
    }

    #[test]
    fn import_lock_releases_after_error() {
        Python::initialize();

        Python::attach(|py| {
            let lock = py
                .import("threading")
                .unwrap()
                .getattr("Lock")
                .unwrap()
                .call0()
                .unwrap();
            let owned = crate::engine::Object::new(lock.clone().unbind());

            let attempt = || -> Result<(), guestpy_core::errors::Error> {
                let _guard = super::PythonImportLock::acquire(py, &owned)?;

                Err(guestpy_core::errors::Error::unexpected("forced failure"))
            };

            assert!(attempt().is_err());

            assert!(
                lock.call_method1("acquire", (false,))
                    .unwrap()
                    .extract::<bool>()
                    .unwrap()
            );

            lock.call_method0("release").unwrap();
        });
    }

    #[test]
    fn ambient_sys_modules_entries_are_not_overwritten() {
        Python::initialize();

        let dotted = unique_name("ambient");
        let prepared = prepared_with(&dotted);

        Python::attach(|py| {
            let modules = PreparedCPythonExtensions::sys_modules(py).unwrap();
            let ambient = PyDict::new(py);

            modules
                .set_item(&dotted, &ambient)
                .unwrap();

            let artifact = prepared
                .bundle
                .modules
                .get(&dotted)
                .unwrap();
            let error = prepared
                .load(NativeExtensionContext::new(py, Vec::new()), &dotted, artifact)
                .unwrap_err();

            assert!(error.to_string().contains(&dotted));
            assert!(
                modules
                    .get_item(&dotted)
                    .unwrap()
                    .is(&ambient)
            );

            modules.del_item(&dotted).unwrap();
        });
    }

    #[test]
    fn successful_execution_keeps_registered_parents() {
        Python::initialize();

        Python::attach(|py| {
            let dotted = unique_name("pkg.native");
            let parent_name = unique_name("pkg");
            let (success, _) = fixture_loaders(py);
            let parent = py
                .import("types")
                .unwrap()
                .getattr("ModuleType")
                .unwrap()
                .call1((&parent_name,))
                .unwrap();
            let context =
                NativeExtensionContext::new(py, vec![(parent_name.clone(), parent.clone())]);
            let module = PreparedCPythonExtensions::execute_loader(
                py,
                &context,
                &dotted,
                std::path::Path::new("/fake/path.so"),
                &success,
            )
            .unwrap();

            assert!(
                module
                    .getattr("marker")
                    .unwrap()
                    .extract::<bool>()
                    .unwrap()
            );

            let modules = PreparedCPythonExtensions::sys_modules(py).unwrap();

            assert!(
                modules
                    .get_item(&parent_name)
                    .unwrap()
                    .is(&parent)
            );
            assert!(
                modules
                    .get_item(&dotted)
                    .unwrap()
                    .is(&module)
            );

            modules.del_item(&dotted).unwrap();
            modules.del_item(&parent_name).unwrap();
        });
    }

    #[test]
    fn matching_parent_objects_are_reused() {
        Python::initialize();

        Python::attach(|py| {
            let dotted = unique_name("pkg.native");
            let parent_name = unique_name("pkg");
            let (success, _) = fixture_loaders(py);
            let parent = py
                .import("types")
                .unwrap()
                .getattr("ModuleType")
                .unwrap()
                .call1((&parent_name,))
                .unwrap();
            let modules = PreparedCPythonExtensions::sys_modules(py).unwrap();

            modules
                .set_item(&parent_name, &parent)
                .unwrap();

            let context =
                NativeExtensionContext::new(py, vec![(parent_name.clone(), parent.clone())]);

            PreparedCPythonExtensions::execute_loader(
                py,
                &context,
                &dotted,
                std::path::Path::new("/fake/path.so"),
                &success,
            )
            .unwrap();

            assert!(
                modules
                    .get_item(&parent_name)
                    .unwrap()
                    .is(&parent)
            );

            modules.del_item(&dotted).unwrap();
            modules.del_item(&parent_name).unwrap();
        });
    }

    #[test]
    fn conflicting_parent_objects_are_rejected() {
        Python::initialize();

        Python::attach(|py| {
            let dotted = unique_name("pkg.native");
            let parent_name = unique_name("pkg");
            let (success, _) = fixture_loaders(py);
            let modules = PreparedCPythonExtensions::sys_modules(py).unwrap();
            let occupant = PyDict::new(py);

            modules
                .set_item(&parent_name, &occupant)
                .unwrap();

            let different_parent = py
                .import("types")
                .unwrap()
                .getattr("ModuleType")
                .unwrap()
                .call1((&parent_name,))
                .unwrap();
            let context =
                NativeExtensionContext::new(py, vec![(parent_name.clone(), different_parent)]);
            let error = PreparedCPythonExtensions::execute_loader(
                py,
                &context,
                &dotted,
                std::path::Path::new("/fake/path.so"),
                &success,
            )
            .unwrap_err();

            assert!(error.to_string().contains(&parent_name));
            assert!(
                modules
                    .get_item(&parent_name)
                    .unwrap()
                    .is(&occupant)
            );
            assert!(modules.get_item(&dotted).is_err());

            modules.del_item(&parent_name).unwrap();
        });
    }

    #[test]
    fn failed_execution_removes_the_child_entry() {
        Python::initialize();

        Python::attach(|py| {
            let dotted = unique_name("pkg.native");
            let (_, failure) = fixture_loaders(py);
            let context = NativeExtensionContext::new(py, Vec::new());

            assert!(
                PreparedCPythonExtensions::execute_loader(
                    py,
                    &context,
                    &dotted,
                    std::path::Path::new("/fake/path.so"),
                    &failure,
                )
                .is_err()
            );

            let modules = PreparedCPythonExtensions::sys_modules(py).unwrap();

            assert!(modules.get_item(&dotted).is_err());
        });
    }

    #[test]
    fn failed_execution_removes_only_inserted_parents() {
        Python::initialize();

        Python::attach(|py| {
            let dotted = unique_name("pkg.native");
            let existing_name = unique_name("pkg.existing");
            let missing_name = unique_name("pkg.missing");
            let (_, failure) = fixture_loaders(py);
            let modules = PreparedCPythonExtensions::sys_modules(py).unwrap();
            let existing_parent = py
                .import("types")
                .unwrap()
                .getattr("ModuleType")
                .unwrap()
                .call1((&existing_name,))
                .unwrap();

            modules
                .set_item(&existing_name, &existing_parent)
                .unwrap();

            let missing_parent = py
                .import("types")
                .unwrap()
                .getattr("ModuleType")
                .unwrap()
                .call1((&missing_name,))
                .unwrap();
            let context = NativeExtensionContext::new(
                py,
                vec![
                    (existing_name.clone(), existing_parent.clone()),
                    (missing_name.clone(), missing_parent),
                ],
            );

            assert!(
                PreparedCPythonExtensions::execute_loader(
                    py,
                    &context,
                    &dotted,
                    std::path::Path::new("/fake/path.so"),
                    &failure,
                )
                .is_err()
            );

            assert!(
                modules
                    .get_item(&existing_name)
                    .unwrap()
                    .is(&existing_parent)
            );
            assert!(modules.get_item(&missing_name).is_err());

            modules
                .del_item(&existing_name)
                .unwrap();
        });
    }

    #[test]
    fn identical_artifact_contents_reuse_the_loaded_module() {
        Python::initialize();

        let dotted = unique_name("pkg.reused");
        let prepared = prepared_with(&dotted);

        Python::attach(|py| {
            let artifact = prepared
                .bundle
                .modules
                .get(&dotted)
                .unwrap();
            let recorded = PyDict::new(py).into_any();

            CPythonNativeExtensions::record_loaded(
                &dotted,
                super::LoadedExtension {
                    origin: artifact.path.clone(),
                    contents: artifact.contents.clone(),
                    module: crate::engine::Object::new(recorded.clone().unbind()),
                },
            )
            .unwrap();

            let module = prepared
                .load(NativeExtensionContext::new(py, Vec::new()), &dotted, artifact)
                .unwrap();

            assert!(module.is(&recorded));
        });
    }

    #[test]
    fn different_artifact_contents_reject_the_loaded_name() {
        Python::initialize();

        let dotted = unique_name("pkg.conflicting");
        let prepared = prepared_with(&dotted);

        Python::attach(|py| {
            let artifact = prepared
                .bundle
                .modules
                .get(&dotted)
                .unwrap();

            CPythonNativeExtensions::record_loaded(
                &dotted,
                super::LoadedExtension {
                    origin: artifact.path.clone(),
                    contents: Arc::from(b"different-bytes".as_slice()),
                    module: crate::engine::Object::new(PyDict::new(py).into_any().unbind()),
                },
            )
            .unwrap();

            let error = prepared
                .load(NativeExtensionContext::new(py, Vec::new()), &dotted, artifact)
                .unwrap_err();

            assert!(error.to_string().contains(&dotted));
        });
    }

    #[test]
    fn incompatible_claims_report_every_candidate() {
        let dotted = unique_name("pkg.incompatible");
        let prepared = PreparedCPythonExtensions {
            bundle: Arc::new(MaterializedBundle {
                root: PathBuf::from("/tmp/guestpy-fixture-root"),
                modules: HashMap::new(),
                incompatible: HashMap::from([(
                    dotted.clone(),
                    vec![
                        format!("{}.cpython-27-x86_64-linux-gnu.so", dotted.replace('.', "/")),
                        format!("{}.cpython-38-x86_64-linux-gnu.so", dotted.replace('.', "/")),
                    ],
                )]),
            }),
        };

        Python::initialize();

        Python::attach(|py| {
            let error = PreparedNativeExtensions::realise(
                &prepared,
                NativeExtensionContext::new(py, Vec::new()),
                &dotted,
            )
            .unwrap_err();

            assert!(error.to_string().contains("cpython-27"));
            assert!(error.to_string().contains("cpython-38"));
        });
    }
}
