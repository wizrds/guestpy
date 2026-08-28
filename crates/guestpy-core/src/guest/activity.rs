use std::{cell::Cell, rc::Rc};

use crate::{backend::Backend, errors::Error, guest::GuestInner, policy::ExecutionPolicy};

#[derive(Copy, Clone)]
pub(crate) enum Activation {
    Operation,
    Cleanup,
}

pub(crate) struct GuestActivity {
    depth: Cell<usize>,
    policy: ExecutionPolicy,
}

impl GuestActivity {
    pub(crate) fn new(policy: ExecutionPolicy) -> Self {
        Self { depth: Cell::new(0), policy }
    }

    fn begin(&self, activation: Activation) -> Result<(), Error> {
        let depth = self.depth.get();

        if depth == 0 && matches!(activation, Activation::Operation) {
            self.policy.begin()?;
        }

        self.depth.set(depth + 1);

        Ok(())
    }

    fn finish(&self) {
        let depth = self
            .depth
            .get()
            .checked_sub(1)
            .expect("active guest depth is balanced");

        self.depth.set(depth);

        if depth == 0 {
            self.policy.disarm();
        }
    }

    pub(crate) fn policy(&self) -> &ExecutionPolicy {
        &self.policy
    }
}

pub(crate) struct ActiveGuest<B: Backend> {
    guest: Rc<GuestInner<B>>,
}

impl<B: Backend> ActiveGuest<B> {
    pub(super) fn new(guest: &Rc<GuestInner<B>>, activation: Activation) -> Result<Self, Error> {
        guest.activity().begin(activation)?;
        guest.runtime().registry().push(guest);

        Ok(Self { guest: guest.clone() })
    }

    pub(crate) fn operation(guest: &Rc<GuestInner<B>>) -> Result<Self, Error> {
        Self::new(guest, Activation::Operation)
    }

    pub(crate) fn cleanup(guest: &Rc<GuestInner<B>>) -> Result<Self, Error> {
        Self::new(guest, Activation::Cleanup)
    }
}

impl<B: Backend> Drop for ActiveGuest<B> {
    fn drop(&mut self) {
        self.guest.runtime().registry().pop();
        self.guest.activity().finish();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Activation, GuestActivity};
    use crate::{
        errors::Error,
        policy::{Cancellation, ExecutionPolicy},
    };

    #[test]
    fn failed_operation_does_not_change_depth() {
        let cancellation = Cancellation::new();

        cancellation.cancel();

        let activity = GuestActivity::new(ExecutionPolicy::new().cancellation(cancellation));

        assert!(matches!(activity.begin(Activation::Operation), Err(Error::Cancelled)));
        assert_eq!(activity.depth.get(), 0);
        assert_eq!(activity.policy.remaining(), None);
    }

    #[test]
    fn nested_operation_keeps_the_outer_deadline() {
        let activity = GuestActivity::new(ExecutionPolicy::new().timeout(Duration::from_secs(1)));

        activity
            .begin(Activation::Operation)
            .unwrap();

        let first = activity.policy.remaining().unwrap();

        activity
            .begin(Activation::Operation)
            .unwrap();

        assert!(activity.policy.remaining().unwrap() <= first);

        activity.finish();

        assert!(activity.policy.remaining().is_some());

        activity.finish();

        assert_eq!(activity.policy.remaining(), None);
    }

    #[test]
    fn cleanup_bypasses_a_cancelled_policy() {
        let cancellation = Cancellation::new();

        cancellation.cancel();

        let activity = GuestActivity::new(ExecutionPolicy::new().cancellation(cancellation));

        activity
            .begin(Activation::Cleanup)
            .unwrap();

        assert_eq!(activity.depth.get(), 1);

        activity.finish();

        assert_eq!(activity.depth.get(), 0);
    }

    #[test]
    fn finish_disarms_only_the_outermost_activity() {
        let activity = GuestActivity::new(ExecutionPolicy::new().timeout(Duration::from_secs(1)));

        activity
            .begin(Activation::Operation)
            .unwrap();
        activity
            .begin(Activation::Operation)
            .unwrap();
        activity.finish();

        assert!(activity.policy.remaining().is_some());

        activity.finish();

        assert_eq!(activity.policy.remaining(), None);
    }
}
