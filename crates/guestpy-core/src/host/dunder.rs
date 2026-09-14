use core::{
    fmt::{Debug, Display, Formatter, Result as FmtResult},
    str::FromStr,
};

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum Dunder {
    Repr,
    Str,
    Bool,
    Hash,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Len,
    GetItem,
    SetItem,
    DelItem,
    Contains,
    Iter,
    Next,
    Call,
    Enter,
    Exit,
    AEnter,
    AExit,
    Aiter,
    Anext,
    Await,
    GetAttr,
    SetAttr,
    DelAttr,
    Add,
    Sub,
    Mul,
    TrueDiv,
    FloorDiv,
    Mod,
    Pow,
    Neg,
    Abs,
    Index,
    Int,
    Float,
    Format,
}

impl Dunder {
    pub fn name(self) -> &'static str {
        match self {
            Self::Repr => "__repr__",
            Self::Str => "__str__",
            Self::Bool => "__bool__",
            Self::Hash => "__hash__",
            Self::Eq => "__eq__",
            Self::Ne => "__ne__",
            Self::Lt => "__lt__",
            Self::Le => "__le__",
            Self::Gt => "__gt__",
            Self::Ge => "__ge__",
            Self::Len => "__len__",
            Self::GetItem => "__getitem__",
            Self::SetItem => "__setitem__",
            Self::DelItem => "__delitem__",
            Self::Contains => "__contains__",
            Self::Iter => "__iter__",
            Self::Next => "__next__",
            Self::Call => "__call__",
            Self::Enter => "__enter__",
            Self::Exit => "__exit__",
            Self::AEnter => "__aenter__",
            Self::AExit => "__aexit__",
            Self::Aiter => "__aiter__",
            Self::Anext => "__anext__",
            Self::Await => "__await__",
            Self::GetAttr => "__getattr__",
            Self::SetAttr => "__setattr__",
            Self::DelAttr => "__delattr__",
            Self::Add => "__add__",
            Self::Sub => "__sub__",
            Self::Mul => "__mul__",
            Self::TrueDiv => "__truediv__",
            Self::FloorDiv => "__floordiv__",
            Self::Mod => "__mod__",
            Self::Pow => "__pow__",
            Self::Neg => "__neg__",
            Self::Abs => "__abs__",
            Self::Index => "__index__",
            Self::Int => "__int__",
            Self::Float => "__float__",
            Self::Format => "__format__",
        }
    }

    pub fn accepts_awaitable(self) -> bool {
        match self {
            Self::Call
            | Self::Eq
            | Self::Ne
            | Self::Lt
            | Self::Le
            | Self::Gt
            | Self::Ge
            | Self::GetItem
            | Self::Next
            | Self::Enter
            | Self::AEnter
            | Self::AExit
            | Self::Anext
            | Self::Await
            | Self::GetAttr
            | Self::Add
            | Self::Sub
            | Self::Mul
            | Self::TrueDiv
            | Self::FloorDiv
            | Self::Mod
            | Self::Pow
            | Self::Neg
            | Self::Abs => true,
            Self::Repr
            | Self::Str
            | Self::Bool
            | Self::Hash
            | Self::Len
            | Self::SetItem
            | Self::DelItem
            | Self::Contains
            | Self::Iter
            | Self::Exit
            | Self::Aiter
            | Self::SetAttr
            | Self::DelAttr
            | Self::Index
            | Self::Int
            | Self::Float
            | Self::Format => false,
        }
    }
}

impl Display for Dunder {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        formatter.write_str(self.name())
    }
}

impl FromStr for Dunder {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(match value {
            "__repr__" => Self::Repr,
            "__str__" => Self::Str,
            "__bool__" => Self::Bool,
            "__hash__" => Self::Hash,
            "__eq__" => Self::Eq,
            "__ne__" => Self::Ne,
            "__lt__" => Self::Lt,
            "__le__" => Self::Le,
            "__gt__" => Self::Gt,
            "__ge__" => Self::Ge,
            "__len__" => Self::Len,
            "__getitem__" => Self::GetItem,
            "__setitem__" => Self::SetItem,
            "__delitem__" => Self::DelItem,
            "__contains__" => Self::Contains,
            "__iter__" => Self::Iter,
            "__next__" => Self::Next,
            "__call__" => Self::Call,
            "__enter__" => Self::Enter,
            "__exit__" => Self::Exit,
            "__aenter__" => Self::AEnter,
            "__aexit__" => Self::AExit,
            "__aiter__" => Self::Aiter,
            "__anext__" => Self::Anext,
            "__await__" => Self::Await,
            "__getattr__" => Self::GetAttr,
            "__setattr__" => Self::SetAttr,
            "__delattr__" => Self::DelAttr,
            "__add__" => Self::Add,
            "__sub__" => Self::Sub,
            "__mul__" => Self::Mul,
            "__truediv__" => Self::TrueDiv,
            "__floordiv__" => Self::FloorDiv,
            "__mod__" => Self::Mod,
            "__pow__" => Self::Pow,
            "__neg__" => Self::Neg,
            "__abs__" => Self::Abs,
            "__index__" => Self::Index,
            "__int__" => Self::Int,
            "__float__" => Self::Float,
            "__format__" => Self::Format,
            _ => return Err(format!("unknown dunder name: {}", value)),
        })
    }
}

#[cfg(test)]
mod tests {
    use core::str::FromStr;

    use super::Dunder;

    #[test]
    fn parses_every_python_dunder_name() {
        let all = [
            Dunder::Repr,
            Dunder::Str,
            Dunder::Bool,
            Dunder::Hash,
            Dunder::Eq,
            Dunder::Ne,
            Dunder::Lt,
            Dunder::Le,
            Dunder::Gt,
            Dunder::Ge,
            Dunder::Len,
            Dunder::GetItem,
            Dunder::SetItem,
            Dunder::DelItem,
            Dunder::Contains,
            Dunder::Iter,
            Dunder::Next,
            Dunder::Call,
            Dunder::Enter,
            Dunder::Exit,
            Dunder::AEnter,
            Dunder::AExit,
            Dunder::Aiter,
            Dunder::Anext,
            Dunder::Await,
            Dunder::GetAttr,
            Dunder::SetAttr,
            Dunder::DelAttr,
            Dunder::Add,
            Dunder::Sub,
            Dunder::Mul,
            Dunder::TrueDiv,
            Dunder::FloorDiv,
            Dunder::Mod,
            Dunder::Pow,
            Dunder::Neg,
            Dunder::Abs,
            Dunder::Index,
            Dunder::Int,
            Dunder::Float,
            Dunder::Format,
        ];

        for dunder in all {
            assert_eq!(Dunder::from_str(dunder.name()), Ok(dunder));
            assert_eq!(dunder.to_string(), dunder.name());
        }
    }

    #[test]
    fn classifies_every_awaitable_result_contract() {
        let cases = [
            (Dunder::Repr, false),
            (Dunder::Str, false),
            (Dunder::Bool, false),
            (Dunder::Hash, false),
            (Dunder::Eq, true),
            (Dunder::Ne, true),
            (Dunder::Lt, true),
            (Dunder::Le, true),
            (Dunder::Gt, true),
            (Dunder::Ge, true),
            (Dunder::Len, false),
            (Dunder::GetItem, true),
            (Dunder::SetItem, false),
            (Dunder::DelItem, false),
            (Dunder::Contains, false),
            (Dunder::Iter, false),
            (Dunder::Next, true),
            (Dunder::Call, true),
            (Dunder::Enter, true),
            (Dunder::Exit, false),
            (Dunder::AEnter, true),
            (Dunder::AExit, true),
            (Dunder::Aiter, false),
            (Dunder::Anext, true),
            (Dunder::Await, true),
            (Dunder::GetAttr, true),
            (Dunder::SetAttr, false),
            (Dunder::DelAttr, false),
            (Dunder::Add, true),
            (Dunder::Sub, true),
            (Dunder::Mul, true),
            (Dunder::TrueDiv, true),
            (Dunder::FloorDiv, true),
            (Dunder::Mod, true),
            (Dunder::Pow, true),
            (Dunder::Neg, true),
            (Dunder::Abs, true),
            (Dunder::Index, false),
            (Dunder::Int, false),
            (Dunder::Float, false),
            (Dunder::Format, false),
        ];

        for (dunder, accepts_awaitable) in cases {
            assert_eq!(dunder.accepts_awaitable(), accepts_awaitable);
        }
    }

    #[test]
    fn rejects_unknown_dunder_names() {
        assert!(Dunder::from_str("__nope__").is_err());
        assert!(Dunder::from_str("repr").is_err());
    }
}
