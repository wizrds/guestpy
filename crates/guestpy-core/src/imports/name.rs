use std::collections::HashSet;

use crate::errors::Error;

#[derive(Copy, Clone)]
pub(super) struct DottedName<'a>(pub(super) &'a str);

impl<'a> DottedName<'a> {
    pub(super) fn absolutise(package: &str, name: &str, level: usize) -> Result<String, Error> {
        if package.is_empty() {
            return Err(Error::import(
                name,
                "attempted relative import with no known parent package",
            ));
        }

        let mut base = package;

        for _ in 1..level {
            base = base
                .rsplit_once('.')
                .map(|(prefix, _)| prefix)
                .ok_or_else(|| {
                    Error::import(name, "attempted relative import beyond top-level package")
                })?;
        }

        if name.is_empty() {
            Ok(base.to_owned())
        } else {
            Ok(format!("{base}.{name}"))
        }
    }

    pub(super) fn head(self) -> &'a str {
        self.0
            .split_once('.')
            .map_or(self.0, |(head, _)| head)
    }

    pub(super) fn parent(self) -> Option<(&'a str, &'a str)> {
        self.0.rsplit_once('.')
    }

    pub(super) fn prefixes(self) -> impl Iterator<Item = &'a str> {
        let dotted = self.0;

        dotted
            .match_indices('.')
            .map(move |(index, _)| &dotted[..index])
            .chain(std::iter::once(dotted))
    }

    pub(super) fn is_denied(self, denied: &HashSet<String>) -> bool {
        self.prefixes()
            .any(|prefix| denied.contains(prefix))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::DottedName;
    use crate::errors::Error;

    struct Denials;

    impl Denials {
        fn of(names: &[&str]) -> HashSet<String> {
            names
                .iter()
                .map(|name| (*name).to_owned())
                .collect()
        }
    }

    #[test]
    fn prefixes_are_shortest_first() {
        assert_eq!(
            DottedName("a.b.c")
                .prefixes()
                .collect::<Vec<_>>(),
            vec!["a", "a.b", "a.b.c"],
        );
        assert_eq!(
            DottedName("a")
                .prefixes()
                .collect::<Vec<_>>(),
            vec!["a"],
        );
    }

    #[test]
    fn head_is_the_first_component() {
        assert_eq!(DottedName("a.b.c").head(), "a");
        assert_eq!(DottedName("a").head(), "a");
    }

    #[test]
    fn parent_splits_the_last_component() {
        assert_eq!(DottedName("a.b.c").parent(), Some(("a.b", "c")));
        assert_eq!(DottedName("a").parent(), None);
    }

    #[test]
    fn a_denied_package_denies_its_submodules() {
        assert!(DottedName("os.path").is_denied(&Denials::of(&["os"])));
        assert!(DottedName("os").is_denied(&Denials::of(&["os"])));
        assert!(!DottedName("ossify").is_denied(&Denials::of(&["os"])));
        assert!(!DottedName("os.path").is_denied(&Denials::of(&["sys"])));
    }

    #[test]
    fn one_level_is_the_package_itself() {
        assert_eq!(
            DottedName::absolutise("plugin.handlers", "util", 1).unwrap(),
            "plugin.handlers.util",
        );
    }

    #[test]
    fn two_levels_is_the_parent_package() {
        assert_eq!(DottedName::absolutise("plugin.handlers", "util", 2).unwrap(), "plugin.util",);
    }

    #[test]
    fn an_empty_name_is_the_package_itself() {
        assert_eq!(DottedName::absolutise("plugin.handlers", "", 1).unwrap(), "plugin.handlers",);
    }

    #[test]
    fn rejects_a_relative_import_beyond_the_top_level() {
        assert!(matches!(
            DottedName::absolutise("plugin", "util", 2),
            Err(Error::Import { ref message, .. })
                if message.contains("beyond top-level package"),
        ));
    }

    #[test]
    fn rejects_a_relative_import_with_no_package() {
        assert!(matches!(
            DottedName::absolutise("", "util", 1),
            Err(Error::Import { ref message, .. })
                if message.contains("no known parent package"),
        ));
    }
}
