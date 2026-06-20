//! Calling an ordinary closure with arguments supplied as an [`HList`].
//!
//! [`HandlerFn`] bridges the heterogeneous argument list accumulated while
//! walking the API description (see [`mod@crate::hlist`]) to a normal positional
//! function call. A blanket impl is generated for every closure arity from 0 up
//! to [`MAX_ARITY`]; each impl destructures the exact `HCons` shape for that
//! arity and forwards to `Fn(A0, A1, ...)`.
//!
//! This is what preserves Servant's guarantee in Rust: the API type determines
//! `Args`, and a handler only type-checks if its parameters line up with `Args`.
//!
//! # Limitation (intentional difference from Haskell)
//!
//! Haskell's `ServerT` currying is unbounded; here arity is capped at
//! [`MAX_ARITY`] because the impls are macro-generated. APIs needing more
//! extracted arguments than that on a single endpoint are not supported; group
//! values into a struct extractor instead.

use crate::hlist::{HCons, HList, HNil};

/// The maximum number of arguments a single handler may take.
pub const MAX_ARITY: usize = 16;

/// A function that can be invoked with its arguments packaged as an [`HList`].
///
/// `Args` is the heterogeneous argument list (computed from the API by
/// [`crate::api::HasArgs`]); `Output` is whatever the underlying closure
/// returns (for a server handler this is a future resolving to a
/// `Result<_, ServerError>`).
pub trait HandlerFn<Args: HList> {
    /// The closure's return type.
    type Output;
    /// Invoke the closure, destructuring `args` into positional parameters.
    fn call_hlist(&self, args: Args) -> Self::Output;
}

macro_rules! impl_handler_fn {
    ( $( $arg:ident ),* ) => {
        impl<Func, Ret, $( $arg ),*> HandlerFn<count_hlist!($( $arg ),*)> for Func
        where
            Func: Fn($( $arg ),*) -> Ret,
        {
            type Output = Ret;
            #[allow(non_snake_case, unused_variables)]
            fn call_hlist(&self, args: count_hlist!($( $arg ),*)) -> Ret {
                destructure_hlist!(args; $( $arg ),*);
                (self)($( $arg ),*)
            }
        }
    };
}

// Build the `HCons<A0, HCons<A1, ... HNil>>` type for a list of idents.
macro_rules! count_hlist {
    () => { HNil };
    ( $head:ident $(, $tail:ident )* ) => {
        HCons<$head, count_hlist!($( $tail ),*)>
    };
}

// Bind each ident to the corresponding element of an `HCons` chain.
macro_rules! destructure_hlist {
    ( $val:ident; ) => {
        let HNil = $val;
    };
    ( $val:ident; $head:ident $(, $tail:ident )* ) => {
        let HCons { head: $head, tail: rest } = $val;
        destructure_hlist!(rest; $( $tail ),*);
    };
}

impl_handler_fn!();
impl_handler_fn!(A0);
impl_handler_fn!(A0, A1);
impl_handler_fn!(A0, A1, A2);
impl_handler_fn!(A0, A1, A2, A3);
impl_handler_fn!(A0, A1, A2, A3, A4);
impl_handler_fn!(A0, A1, A2, A3, A4, A5);
impl_handler_fn!(A0, A1, A2, A3, A4, A5, A6);
impl_handler_fn!(A0, A1, A2, A3, A4, A5, A6, A7);
impl_handler_fn!(A0, A1, A2, A3, A4, A5, A6, A7, A8);
impl_handler_fn!(A0, A1, A2, A3, A4, A5, A6, A7, A8, A9);
impl_handler_fn!(A0, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10);
impl_handler_fn!(A0, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11);
impl_handler_fn!(A0, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12);
impl_handler_fn!(A0, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13);
impl_handler_fn!(
    A0, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14
);
impl_handler_fn!(
    A0, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11, A12, A13, A14, A15
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hlist::hlist1;

    #[test]
    fn arity_0() {
        let f = || 42u32;
        assert_eq!(HandlerFn::call_hlist(&f, HNil), 42);
    }

    #[test]
    fn arity_1() {
        let f = |x: u32| x + 1;
        assert_eq!(f.call_hlist(hlist1(41u32)), 42);
    }

    #[test]
    fn arity_3_preserves_order() {
        let f = |a: u32, b: &str, c: bool| format!("{a}-{b}-{c}");
        let args = HCons {
            head: 7u32,
            tail: HCons {
                head: "k",
                tail: hlist1(true),
            },
        };
        assert_eq!(f.call_hlist(args), "7-k-true");
    }
}
