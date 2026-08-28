use std::ops::{Deref, DerefMut};

pub struct GilSerialized<T>(T);

impl<T> GilSerialized<T> {
    pub fn new(value: T) -> Self {
        Self(value)
    }

    pub fn as_inner(&self) -> &T {
        &self.0
    }

    pub fn as_inner_mut(&mut self) -> &mut T {
        &mut self.0
    }

    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> Deref for GilSerialized<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.as_inner()
    }
}

impl<T> DerefMut for GilSerialized<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_inner_mut()
    }
}

impl<T> From<T> for GilSerialized<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<T> AsRef<T> for GilSerialized<T> {
    fn as_ref(&self) -> &T {
        self.as_inner()
    }
}

// SAFETY: Every read, call, and drop of `T` happens while this
// thread holds the CPython GIL. Acquire/release is a real lock,
// so two threads never touch the same value at the same time.
unsafe impl<T> Send for GilSerialized<T> {}
unsafe impl<T> Sync for GilSerialized<T> {}
