use std::collections::HashSet;

use crate::{
    backend::{Backend, Tok, Val},
    bundle::Bundle,
    errors::Error,
};

pub struct NativeExtensionContext<'py, B: Backend> {
    token: Tok<'py, B>,
    parents: Vec<(String, Val<'py, B>)>,
}

impl<'py, B: Backend> NativeExtensionContext<'py, B> {
    pub fn new(token: Tok<'py, B>, parents: Vec<(String, Val<'py, B>)>) -> Self {
        Self { token, parents }
    }

    pub fn token(&self) -> Tok<'py, B> {
        self.token
    }

    pub fn parents(&self) -> impl Iterator<Item = (&str, &Val<'py, B>)> {
        self.parents
            .iter()
            .map(|(name, value)| (name.as_str(), value))
    }
}

pub trait NativeExtensionLoader<B: Backend> {
    type Prepared: PreparedNativeExtensions<B> + 'static;

    fn prepare<'py>(token: Tok<'py, B>, bundle: &Bundle) -> Result<Self::Prepared, Error>;
}

pub trait PreparedNativeExtensions<B: Backend> {
    fn names(&self) -> impl Iterator<Item = &str>;

    fn realise<'py>(
        &self,
        context: NativeExtensionContext<'py, B>,
        dotted: &str,
    ) -> Result<Val<'py, B>, Error>;

    fn source_origin(&self, relative: &str) -> Option<String>;

    fn package_path(&self, dotted: &str) -> Option<String>;
}

pub(crate) type PreparedNativeExtensionsOf<B> =
    <<B as Backend>::NativeExtensions as NativeExtensionLoader<B>>::Prepared;

pub struct NoNativeExtensions;

pub struct UnsupportedNativeExtensions {
    claims: HashSet<String>,
}

impl UnsupportedNativeExtensions {
    fn is_identifier(name: &str) -> bool {
        let mut characters = name.chars();

        matches!(characters.next(), Some(character) if character == '_' || character.is_ascii_alphabetic())
            && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
    }

    fn claim(path: &str) -> Option<String> {
        let normalized = path.replace('\\', "/");
        let stripped = normalized
            .strip_suffix(".so")
            .or_else(|| normalized.strip_suffix(".pyd"))
            .or_else(|| normalized.strip_suffix(".dylib"))?;

        let mut parts = stripped
            .split('/')
            .collect::<Vec<_>>();
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
}

impl<B: Backend> NativeExtensionLoader<B> for NoNativeExtensions {
    type Prepared = UnsupportedNativeExtensions;

    fn prepare<'py>(_: Tok<'py, B>, bundle: &Bundle) -> Result<Self::Prepared, Error> {
        Ok(UnsupportedNativeExtensions {
            claims: bundle
                .files()
                .filter_map(|(path, _)| UnsupportedNativeExtensions::claim(path))
                .collect(),
        })
    }
}

impl<B: Backend> PreparedNativeExtensions<B> for UnsupportedNativeExtensions {
    fn names(&self) -> impl Iterator<Item = &str> {
        self.claims
            .iter()
            .map(String::as_str)
    }

    fn realise<'py>(
        &self,
        _: NativeExtensionContext<'py, B>,
        dotted: &str,
    ) -> Result<Val<'py, B>, Error> {
        Err(Error::unsupported(format!(
            "backend {} cannot load native extension module {dotted}",
            B::NAME,
        )))
    }

    fn source_origin(&self, _: &str) -> Option<String> {
        None
    }

    fn package_path(&self, _: &str) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{
        NativeExtensionLoader,
        PreparedNativeExtensions,
        UnsupportedNativeExtensions,
        NoNativeExtensions,
        NativeExtensionContext,
    };
    use crate::{backend::tests::Stub, bundle::Bundle};

    #[test]
    fn identifies_a_simple_native_module() {
        assert_eq!(
            UnsupportedNativeExtensions::claim("module.so").as_deref(),
            Some("module"),
        );
    }

    #[test]
    fn removes_a_conservative_abi_suffix() {
        assert_eq!(
            UnsupportedNativeExtensions::claim(
                "package/_native.cpython-313-x86_64-linux-gnu.so",
            )
            .as_deref(),
            Some("package._native"),
        );
    }

    #[test]
    fn ignores_non_identifier_dependency_directories() {
        assert_eq!(UnsupportedNativeExtensions::claim(".libs/libdependency.so"), None);
    }

    #[test]
    fn ignores_python_and_data_files() {
        assert_eq!(UnsupportedNativeExtensions::claim("module.py"), None);
        assert_eq!(UnsupportedNativeExtensions::claim("data.json"), None);
    }

    #[test]
    fn unsupported_realisation_names_the_backend_and_module() {
        let bundle = Bundle::builder()
            .data("package/_native.cpython-313-x86_64-linux-gnu.so", b"".to_vec())
            .package("package", "")
            .build()
            .unwrap();
        let prepared =
            <NoNativeExtensions as NativeExtensionLoader<Stub>>::prepare((), &bundle)
                .unwrap();
        let error = prepared
            .realise(
                NativeExtensionContext::<Stub>::new((), Vec::new()),
                "package._native",
            )
            .unwrap_err();

        assert!(error.to_string().contains("stub"));
        assert!(error.to_string().contains("package._native"));
    }

    #[test]
    fn unsupported_preparation_has_no_materialized_paths() {
        let bundle = Bundle::builder()
            .data("package/_native.cpython-313-x86_64-linux-gnu.so", b"".to_vec())
            .package("package", "")
            .build()
            .unwrap();
        let prepared =
            <NoNativeExtensions as NativeExtensionLoader<Stub>>::prepare((), &bundle)
                .unwrap();

        assert_eq!(
            PreparedNativeExtensions::<Stub>::source_origin(
                &prepared,
                "package/_native.cpython-313-x86_64-linux-gnu.so",
            ),
            None,
        );
        assert_eq!(
            PreparedNativeExtensions::<Stub>::package_path(&prepared, "package._native"),
            None,
        );
    }
}
