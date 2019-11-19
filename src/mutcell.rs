use std::fmt::Debug;
use std::hash::{Hash, Hasher};

/// Contains a value that can be updated via move.
/// See `MutCell::map_value` in particular.
pub struct MutCell<T>(State<T>);

enum State<T> {
    Poisoned,
    Value(T),
}

impl<T> MutCell<T> {
    /// Move `v` into a new `MutCell`.
    pub fn new(v: T) -> Self {
        Self(State::Value(v))
    }

    /// Move the contained value out.
    pub fn into_inner(self) -> T {
        use State::*;
        match self.0 {
            Poisoned => panic!("poisoned"),
            Value(v) => v,
        }
    }

    /// Returns true if the `MutCell` has been poisoned.
    pub fn is_poisoned(&self) -> bool {
        use State::*;
        match self.0 {
            Poisoned => true,
            Value(_) => false,
        }
    }

    /// Use the given function to update the inner value.
    ///
    /// Note that if `f` panics, then the `MutCell` will be poisoned, and futher
    /// uses of it will themselves panic.
    pub fn map_value<F>(&mut self, f: F)
    where
        F: FnOnce(T) -> T,
    {
        use State::*;
        self.0 = match std::mem::replace(&mut self.0, State::Poisoned) {
            Poisoned => panic!("poisoned"),
            Value(v) => Value(f(v)),
        };
    }
}

impl<T> AsMut<T> for MutCell<T> {
    fn as_mut(&mut self) -> &mut T {
        use State::*;
        match &mut self.0 {
            Poisoned => panic!("poisoned"),
            Value(v) => v,
        }
    }
}

impl<T> AsRef<T> for MutCell<T> {
    fn as_ref(&self) -> &T {
        use State::*;
        match &self.0 {
            Poisoned => panic!("poisoned"),
            Value(v) => v,
        }
    }
}

impl<T> Clone for MutCell<T>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        MutCell(State::Value(self.as_ref().clone()))
    }
}

impl<T> Debug for MutCell<T>
where
    T: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        use State::*;
        match &self.0 {
            Poisoned => write!(f, "MutCell(<poisoned>)"),
            Value(v) => write!(f, "MutCell({:?})", v),
        }
    }
}

impl<T> Default for MutCell<T>
where
    T: Default,
{
    fn default() -> Self {
        MutCell(State::Value(T::default()))
    }
}

impl<T> Eq for MutCell<T> where T: Eq {}

impl<T> Hash for MutCell<T>
where
    T: Hash,
{
    fn hash<H>(&self, h: &mut H)
    where
        H: Hasher,
    {
        self.as_ref().hash(h)
    }
}

impl<T> PartialEq for MutCell<T>
where
    T: PartialEq,
{
    fn eq(&self, rhs: &Self) -> bool {
        self.as_ref() == rhs.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Eq, PartialEq)]
    enum TestEnum {
        One(i64),
        Two(i64),
    }

    impl TestEnum {
        fn flip(self) -> Self {
            use TestEnum::*;
            match self {
                One(v) => Two(v),
                Two(v) => One(v),
            }
        }

        fn inc(&mut self) {
            use TestEnum::*;
            match self {
                One(ref mut v) => *v += 1,
                Two(ref mut v) => *v += 1,
            }
        }
    }

    #[test]
    fn mut_cell() {
        use TestEnum::*;

        let mut cell = MutCell::new(One(5));
        assert_eq!(cell.as_ref(), &One(5));
        cell.map_value(TestEnum::flip);
        assert_eq!(cell.as_ref(), &Two(5));

        cell.as_mut().inc();
        assert_eq!(cell.as_ref(), &Two(6));

        let v = cell.into_inner();
        assert_eq!(v, Two(6));
    }
}
