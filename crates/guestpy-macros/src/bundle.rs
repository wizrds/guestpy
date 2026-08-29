use std::path::PathBuf;

use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Ident,
    LitStr,
    Path,
    Token,
    parse::{Parse, ParseStream},
};

use crate::path::CratePath;

struct BundleInput {
    path: LitStr,
    crate_path: Option<Path>,
}

impl Parse for BundleInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let path = input.parse()?;

        if input.is_empty() {
            return Ok(Self {
                path,
                crate_path: None,
            });
        }

        input.parse::<Token![,]>()?;

        if input.is_empty() {
            return Ok(Self {
                path,
                crate_path: None,
            });
        }

        let option = input.parse::<Ident>()?;

        if option != "crate_path" {
            return Err(syn::Error::new(
                option.span(),
                "expected `crate_path`",
            ));
        }

        input.parse::<Token![=]>()?;

        let crate_path = input.parse()?;

        if !input.is_empty() {
            input.parse::<Token![,]>()?;
        }

        if !input.is_empty() {
            return Err(input.error("unexpected bundle option"));
        }

        Ok(Self {
            path,
            crate_path: Some(crate_path),
        })
    }
}

#[derive(Debug)]
pub(crate) struct BundleMacro {
    path: LitStr,
    root: LitStr,
    crate_path: Path,
}

impl BundleMacro {
    pub(crate) fn new(tokens: TokenStream) -> syn::Result<Self> {
        let BundleInput {
            path,
            crate_path,
        } = syn::parse2(tokens)?;
        let resolved = Self::resolve_path(&path)?;
        let root = resolved
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                syn::Error::new(
                    path.span(),
                    "embedded bundle path has no UTF-8 directory name",
                )
            })?;

        Ok(Self {
            root: LitStr::new(root, path.span()),
            crate_path: CratePath::new(crate_path).resolve(),
            path,
        })
    }

    fn resolve_path(path: &LitStr) -> syn::Result<PathBuf> {
        let value = path.value();
        let mut remaining = value.as_str();
        let mut resolved = String::new();

        while let Some(index) = remaining.find('$') {
            let (head, tail) = remaining.split_at(index);

            resolved.push_str(head);

            let Some((variable, rest)) = Self::variable(&tail[1..]) else {
                return Err(syn::Error::new(
                    path.span(),
                    format!(
                        "unable to parse environment variable in {tail:?}"
                    ),
                ));
            };
            let replacement = std::env::var(variable).map_err(|_| {
                syn::Error::new(
                    path.span(),
                    format!(
                        "environment variable {variable:?} is not defined"
                    ),
                )
            })?;

            resolved.push_str(&replacement);
            remaining = rest;
        }

        resolved.push_str(remaining);

        Ok(PathBuf::from(resolved))
    }

    fn variable(value: &str) -> Option<(&str, &str)> {
        let mut end = 0;

        for (index, character) in value.char_indices() {
            let valid = if index == 0 {
                character == '_' || character.is_ascii_alphabetic()
            } else {
                character == '_'
                    || character.is_ascii_alphabetic()
                    || character.is_ascii_digit()
            };

            if !valid {
                break;
            }

            end = index + character.len_utf8();
        }

        (end != 0).then(|| value.split_at(end))
    }

    pub(crate) fn expand(self) -> TokenStream {
        let Self {
            path,
            root,
            crate_path,
        } = self;

        quote! {{
            use #crate_path::embed::__include_dir as include_dir;

            const __GUESTPY_EMBEDDED: #crate_path::embed::Dir<'static> =
                #crate_path::embed::__include_dir_macro!(#path);

            #crate_path::bundle::Bundle::from_embedded(
                &#crate_path::embed::Dir::new(
                    #root,
                    __GUESTPY_EMBEDDED.entries(),
                ),
            )
        }}
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::BundleMacro;

    #[test]
    fn restores_the_embedded_directory_name() {
        let output = BundleMacro::new(quote!("fixtures/plugin"))
            .unwrap()
            .expand()
            .to_string();

        assert!(output.contains("Dir :: new (\"plugin\""));
        assert!(output.contains("Bundle :: from_embedded"));
    }

    #[test]
    fn preserves_an_explicit_crate_path() {
        let output = BundleMacro::new(quote!(
            "fixtures/plugin",
            crate_path = custom::guestpy,
        ))
        .unwrap()
        .expand()
        .to_string();

        assert!(output.contains(
            "use custom :: guestpy :: embed :: __include_dir as include_dir",
        ));
        assert!(output.contains(
            "custom :: guestpy :: embed :: Dir < 'static >",
        ));
        assert!(output.contains(
            "custom :: guestpy :: embed :: __include_dir_macro !",
        ));
        assert!(output.contains(
            "custom :: guestpy :: bundle :: Bundle :: from_embedded",
        ));
    }

    #[test]
    fn accepts_a_trailing_comma() {
        BundleMacro::new(quote!("fixtures/plugin",))
            .unwrap();
    }

    #[test]
    fn rejects_an_unknown_option() {
        let error = BundleMacro::new(quote!(
            "fixtures/plugin",
            guestpy_path = custom::guestpy,
        ))
        .unwrap_err();

        assert_eq!(error.to_string(), "expected `crate_path`");
    }

    #[test]
    fn rejects_a_duplicate_crate_path() {
        let error = BundleMacro::new(quote!(
            "fixtures/plugin",
            crate_path = first::guestpy,
            crate_path = second::guestpy,
        ))
        .unwrap_err();

        assert_eq!(error.to_string(), "unexpected bundle option");
    }

    #[test]
    fn rejects_a_missing_environment_variable() {
        let error = BundleMacro::new(
            quote!("$GUESTPY_BUNDLE_MISSING_VARIABLE/plugin"),
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("GUESTPY_BUNDLE_MISSING_VARIABLE"),
        );
    }

    #[test]
    fn rejects_a_path_without_a_directory_name() {
        let error = BundleMacro::new(quote!("/"))
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("path has no UTF-8 directory name"),
        );
    }
}
