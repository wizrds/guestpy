use std::{borrow::Cow, fmt};

pub trait Describe {
    fn describe(expected: &mut Expected);
}

pub struct Expected {
    names: Vec<Cow<'static, str>>,
}

impl Expected {
    pub fn of<T: Describe + ?Sized>() -> Self {
        let mut expected = Self { names: Vec::new() };

        T::describe(&mut expected);

        expected
    }

    pub fn push(&mut self, name: impl Into<Cow<'static, str>>) {
        let name = name.into();

        if !self.names.contains(&name) {
            self.names.push(name);
        }
    }
}

impl fmt::Display for Expected {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.names.split_last() {
            None => formatter.write_str("nothing"),
            Some((last, [])) => formatter.write_str(last),
            Some((last, rest)) => write!(formatter, "{} or {last}", rest.join(", ")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Describe, Expected};

    struct Empty;

    impl Describe for Empty {
        fn describe(_expected: &mut Expected) {}
    }

    struct Names;

    impl Describe for Names {
        fn describe(expected: &mut Expected) {
            expected.push("first");
            expected.push("second");
            expected.push("second");
            expected.push("third");
        }
    }

    #[test]
    fn expected_renders_an_empty_description() {
        assert_eq!(Expected::of::<Empty>().to_string(), "nothing");
    }

    #[test]
    fn expected_renders_one_name_without_a_conjunction() {
        assert_eq!(Expected::of::<i64>().to_string(), "int");
    }

    #[test]
    fn expected_preserves_order_and_removes_duplicate_names() {
        assert_eq!(Expected::of::<Names>().to_string(), "first, second or third",);
    }
}
