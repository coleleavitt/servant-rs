use std::marker::PhantomData;

use super::combinators::{Header, QueryParam, ReqBody};
use crate::modifiers::{Lenient, Optional, Required, Strict};

// ---------------------------------------------------------------------------
// Modifier builder methods (last-wins via ordinary method chaining)
// ---------------------------------------------------------------------------

impl<A, P, S, Next> QueryParam<A, P, S, Next> {
    /// Make the parameter required (absence rejects the request).
    pub fn required(self) -> QueryParam<A, Required, S, Next> {
        QueryParam {
            name: self.name,
            next: self.next,
            _marker: PhantomData,
        }
    }
    /// Make the parameter optional (absence yields `None`).
    pub fn optional(self) -> QueryParam<A, Optional, S, Next> {
        QueryParam {
            name: self.name,
            next: self.next,
            _marker: PhantomData,
        }
    }
    /// Reject the request on a parse failure (strict).
    pub fn strict(self) -> QueryParam<A, P, Strict, Next> {
        QueryParam {
            name: self.name,
            next: self.next,
            _marker: PhantomData,
        }
    }
    /// Surface parse failures to the handler as `Err` (lenient).
    pub fn lenient(self) -> QueryParam<A, P, Lenient, Next> {
        QueryParam {
            name: self.name,
            next: self.next,
            _marker: PhantomData,
        }
    }
}

impl<A, P, S, Next> Header<A, P, S, Next> {
    /// Make the header required.
    pub fn required(self) -> Header<A, Required, S, Next> {
        Header {
            name: self.name,
            next: self.next,
            _marker: PhantomData,
        }
    }
    /// Make the header optional.
    pub fn optional(self) -> Header<A, Optional, S, Next> {
        Header {
            name: self.name,
            next: self.next,
            _marker: PhantomData,
        }
    }
    /// Reject the request on a parse failure (strict).
    pub fn strict(self) -> Header<A, P, Strict, Next> {
        Header {
            name: self.name,
            next: self.next,
            _marker: PhantomData,
        }
    }
    /// Surface parse failures to the handler as `Err` (lenient).
    pub fn lenient(self) -> Header<A, P, Lenient, Next> {
        Header {
            name: self.name,
            next: self.next,
            _marker: PhantomData,
        }
    }
}

impl<CTypes, A, S, Next> ReqBody<CTypes, A, S, Next> {
    /// Reject the request on a body parse failure (strict).
    pub fn strict(self) -> ReqBody<CTypes, A, Strict, Next> {
        ReqBody {
            next: self.next,
            _marker: PhantomData,
        }
    }
    /// Surface a body parse failure to the handler as `Err` (lenient).
    pub fn lenient(self) -> ReqBody<CTypes, A, Lenient, Next> {
        ReqBody {
            next: self.next,
            _marker: PhantomData,
        }
    }
}
