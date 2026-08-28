use super::{Backend, Step, Tok, Val};
use crate::errors::Error;

pub trait BackendValues: Backend {
    fn none<'py>(token: Tok<'py, Self>) -> Val<'py, Self>;
    fn bool<'py>(token: Tok<'py, Self>, value: bool) -> Val<'py, Self>;
    fn int<'py>(token: Tok<'py, Self>, value: i64) -> Val<'py, Self>;
    fn uint<'py>(token: Tok<'py, Self>, value: u64) -> Val<'py, Self>;
    fn float<'py>(token: Tok<'py, Self>, value: f64) -> Val<'py, Self>;
    fn str<'py>(token: Tok<'py, Self>, value: &str) -> Val<'py, Self>;
    fn bytes<'py>(token: Tok<'py, Self>, value: &[u8]) -> Val<'py, Self>;
    fn list<'py>(
        token: Tok<'py, Self>,
        items: Vec<Val<'py, Self>>,
    ) -> Result<Val<'py, Self>, Error>;
    fn tuple<'py>(
        token: Tok<'py, Self>,
        items: Vec<Val<'py, Self>>,
    ) -> Result<Val<'py, Self>, Error>;
    fn dict<'py>(
        token: Tok<'py, Self>,
        pairs: Vec<(Val<'py, Self>, Val<'py, Self>)>,
    ) -> Result<Val<'py, Self>, Error>;
    fn set<'py>(token: Tok<'py, Self>, items: Vec<Val<'py, Self>>)
    -> Result<Val<'py, Self>, Error>;
    fn new_dict<'py>(token: Tok<'py, Self>) -> Result<Val<'py, Self>, Error>;
    fn is_bool<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> bool;
    fn is_int<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> bool;
    fn is_float<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> bool;
    fn is_str<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> bool;
    fn is_bytes<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> bool;
    fn is_list<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> bool;
    fn is_tuple<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> bool;
    fn is_dict<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> bool;
    fn is_set<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> bool;
    fn is_callable<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> bool;
    fn is_class<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> bool;
    fn is_instance<'py>(
        token: Tok<'py, Self>,
        value: &Val<'py, Self>,
        class: &Val<'py, Self>,
    ) -> Result<bool, Error>;
    fn is_subclass<'py>(
        token: Tok<'py, Self>,
        first: &Val<'py, Self>,
        second: &Val<'py, Self>,
    ) -> Result<bool, Error>;
    fn is_iterable<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> bool;
    fn is_none<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> bool;
    fn as_bool<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<bool, Error>;
    fn as_i64<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<i64, Error>;
    fn as_u64<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<u64, Error>;
    fn as_f64<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<f64, Error>;
    fn as_str<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<String, Error>;
    fn as_bytes<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<Vec<u8>, Error>;
    fn len<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<usize, Error>;
    fn type_name<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> String;
    fn identity<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> usize;
    fn truthy<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<bool, Error>;
    fn repr<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<String, Error>;
    fn display<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<String, Error>;
    fn dir<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<Vec<String>, Error>;
    fn get_attr<'py>(
        token: Tok<'py, Self>,
        value: &Val<'py, Self>,
        name: &str,
    ) -> Result<Val<'py, Self>, Error>;
    fn set_attr<'py>(
        token: Tok<'py, Self>,
        value: &Val<'py, Self>,
        name: &str,
        attribute: Val<'py, Self>,
    ) -> Result<(), Error>;
    fn del_attr<'py>(
        token: Tok<'py, Self>,
        value: &Val<'py, Self>,
        name: &str,
    ) -> Result<(), Error>;
    fn has_attr<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>, name: &str) -> bool;
    fn get_item<'py>(
        token: Tok<'py, Self>,
        value: &Val<'py, Self>,
        key: &Val<'py, Self>,
    ) -> Result<Val<'py, Self>, Error>;
    fn get_item_opt<'py>(
        token: Tok<'py, Self>,
        value: &Val<'py, Self>,
        key: &Val<'py, Self>,
    ) -> Result<Option<Val<'py, Self>>, Error>;
    fn set_item<'py>(
        token: Tok<'py, Self>,
        value: &Val<'py, Self>,
        key: Val<'py, Self>,
        item: Val<'py, Self>,
    ) -> Result<(), Error>;
    fn del_item<'py>(
        token: Tok<'py, Self>,
        value: &Val<'py, Self>,
        key: &Val<'py, Self>,
    ) -> Result<(), Error>;
    fn copy_dict<'py>(
        token: Tok<'py, Self>,
        value: &Val<'py, Self>,
    ) -> Result<Val<'py, Self>, Error>;
    fn call<'py>(
        token: Tok<'py, Self>,
        callable: &Val<'py, Self>,
        args: &[Val<'py, Self>],
        kwargs: &[(&str, Val<'py, Self>)],
    ) -> Result<Val<'py, Self>, Error>;
    fn iter<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<Val<'py, Self>, Error>;
    fn next<'py>(
        token: Tok<'py, Self>,
        iterator: &Val<'py, Self>,
    ) -> Result<Option<Val<'py, Self>>, Error>;
    fn send<'py>(
        token: Tok<'py, Self>,
        generator: &Val<'py, Self>,
        value: Val<'py, Self>,
    ) -> Result<Step<Val<'py, Self>>, Error>;
    fn throw<'py>(
        token: Tok<'py, Self>,
        generator: &Val<'py, Self>,
        exception: Val<'py, Self>,
    ) -> Result<Step<Val<'py, Self>>, Error>;
    fn close<'py>(token: Tok<'py, Self>, generator: &Val<'py, Self>) -> Result<(), Error>;
}
