use std::{
    cell::RefCell,
    collections::HashMap,
    rc::{Rc, Weak},
};

use crate::{
    backend::Backend,
    errors::Error,
    guest::{GuestId, GuestInner},
};

pub(crate) struct GuestRegistry<B: Backend> {
    registered: RefCell<HashMap<GuestId, Weak<GuestInner<B>>>>,
    active: RefCell<Vec<Weak<GuestInner<B>>>>,
}

impl<B: Backend> GuestRegistry<B> {
    pub(crate) fn new() -> Self {
        Self {
            registered: RefCell::new(HashMap::new()),
            active: RefCell::new(Vec::new()),
        }
    }

    pub(crate) fn register(&self, guest: &Rc<GuestInner<B>>) {
        self.registered
            .borrow_mut()
            .insert(guest.id(), Rc::downgrade(guest));
    }

    pub(crate) fn unregister(&self, id: GuestId) {
        self.registered.borrow_mut().remove(&id);
    }

    pub(crate) fn get(&self, id: GuestId) -> Result<Rc<GuestInner<B>>, Error> {
        let guest = self
            .registered
            .borrow()
            .get(&id)
            .and_then(Weak::upgrade);

        if guest.is_none() {
            self.registered.borrow_mut().remove(&id);
        }

        guest.ok_or(Error::Closed)
    }

    pub(crate) fn is_innermost(&self, guest: &Rc<GuestInner<B>>) -> bool {
        self.innermost()
            .is_some_and(|active| Rc::ptr_eq(&active, guest))
    }

    pub(crate) fn innermost(&self) -> Option<Rc<GuestInner<B>>> {
        self.active
            .borrow()
            .last()
            .and_then(Weak::upgrade)
    }

    pub(crate) fn push(&self, guest: &Rc<GuestInner<B>>) {
        self.active
            .borrow_mut()
            .push(Rc::downgrade(guest));
    }

    pub(crate) fn pop(&self) {
        self.active
            .borrow_mut()
            .pop()
            .expect("active guest stack is balanced");
    }
}
