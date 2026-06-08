//! A minimal heterogeneous list used to accumulate extracted handler arguments.
//!
//! Servant computes a handler's argument list at the type level by folding the
//! API description. Rust has no type-level function that builds an
//! arbitrary-arity function type, so we accumulate an [`HCons`]/[`HNil`] list
//! while walking the combinators and convert it into a positional call at the
//! edge (see [`crate::func`]). The list is an implementation detail; users never
//! name it directly — they write ordinary closures whose parameters line up with
//! the API description.

/// Marker trait implemented by [`HNil`] and [`HCons`].
pub trait HList: Sized {
    /// Number of elements in the list.
    const LEN: usize;
}

/// The empty heterogeneous list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HNil;

/// A non-empty heterogeneous list: a `head` element prepended to a `tail` list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HCons<H, T: HList> {
    /// The first element.
    pub head: H,
    /// The remaining elements.
    pub tail: T,
}

impl HList for HNil {
    const LEN: usize = 0;
}

impl<H, T: HList> HList for HCons<H, T> {
    const LEN: usize = 1 + T::LEN;
}

/// Convenience constructor for a one-element list.
pub fn hlist1<A>(a: A) -> HCons<A, HNil> {
    HCons {
        head: a,
        tail: HNil,
    }
}

/// Build an [`HList`] from a comma-separated list of values, e.g.
/// `hlist![42u64, true, Some("x")]`. Used to pass arguments to typed clients
/// and link builders.
#[macro_export]
macro_rules! hlist {
    () => { $crate::hlist::HNil };
    ($head:expr $(, $tail:expr)* $(,)?) => {
        $crate::hlist::HCons { head: $head, tail: $crate::hlist!($($tail),*) }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn len_is_computed_at_type_level() {
        assert_eq!(<HNil as HList>::LEN, 0);
        assert_eq!(<HCons<u8, HNil> as HList>::LEN, 1);
        assert_eq!(<HCons<u8, HCons<&str, HNil>> as HList>::LEN, 2);
    }

    #[test]
    fn construct_and_destructure() {
        let l = HCons {
            head: 1u32,
            tail: HCons {
                head: "x",
                tail: HNil,
            },
        };
        assert_eq!(l.head, 1);
        assert_eq!(l.tail.head, "x");
    }
}
