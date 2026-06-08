//! The server [`Context`] — Servant's `Context '[...]`: a typemap of
//! application-level values the extraction phase needs but the request does not
//! carry (e.g. the basic-auth check, per-request resource providers).

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;

use servant::auth::{BasicAuthData, BasicAuthResult};
use servant::error::ServerError;

/// A heterogeneous, type-keyed bag of context entries supplied at `serve` time.
#[derive(Default)]
pub struct Context {
    entries: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl Context {
    /// An empty context.
    pub fn new() -> Self {
        Context::default()
    }

    /// Insert an entry keyed by its type.
    pub fn insert<T: Any + Send + Sync>(&mut self, value: T) {
        self.entries.insert(TypeId::of::<T>(), Box::new(value));
    }

    /// Builder form of [`Context::insert`].
    pub fn with<T: Any + Send + Sync>(mut self, value: T) -> Self {
        self.insert(value);
        self
    }

    /// Look up an entry by type.
    pub fn get<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.entries
            .get(&TypeId::of::<T>())
            .and_then(|b| b.downcast_ref::<T>())
    }
}

/// A basic-auth check supplied via [`Context`]: maps decoded credentials to a
/// [`BasicAuthResult`] for the user type `Usr`.
pub struct BasicAuthCheck<Usr>(
    #[allow(clippy::type_complexity)]
    pub  Arc<dyn Fn(&BasicAuthData) -> BasicAuthResult<Usr> + Send + Sync>,
);

impl<Usr> BasicAuthCheck<Usr> {
    /// Build a check from a closure.
    pub fn new(f: impl Fn(&BasicAuthData) -> BasicAuthResult<Usr> + Send + Sync + 'static) -> Self {
        BasicAuthCheck(Arc::new(f))
    }
}

impl<Usr> Clone for BasicAuthCheck<Usr> {
    fn clone(&self) -> Self {
        BasicAuthCheck(self.0.clone())
    }
}

/// A per-request resource provider supplied via [`Context`], used by
/// `WithResource`.
pub struct ResourceProvider<R>(pub Arc<dyn Fn() -> R + Send + Sync>);

impl<R> ResourceProvider<R> {
    /// Build a provider from a closure.
    pub fn new(f: impl Fn() -> R + Send + Sync + 'static) -> Self {
        ResourceProvider(Arc::new(f))
    }
}

impl<R> Clone for ResourceProvider<R> {
    fn clone(&self) -> Self {
        ResourceProvider(self.0.clone())
    }
}

/// A generalized authentication check (`AuthProtect`): resolves a user `Usr`
/// from the request headers, or returns the error to send (e.g. `401`/`403`).
pub struct AuthCheck<Usr>(
    #[allow(clippy::type_complexity)]
    pub  Arc<dyn Fn(&http::HeaderMap) -> Result<Usr, ServerError> + Send + Sync>,
);

impl<Usr> AuthCheck<Usr> {
    /// Build a check from a closure.
    pub fn new(
        f: impl Fn(&http::HeaderMap) -> Result<Usr, ServerError> + Send + Sync + 'static,
    ) -> Self {
        AuthCheck(Arc::new(f))
    }
}

impl<Usr> Clone for AuthCheck<Usr> {
    fn clone(&self) -> Self {
        AuthCheck(self.0.clone())
    }
}

/// A named sub-context (for `WithNamedContext`), keyed in a parent [`Context`]
/// by the marker type `Name`.
pub struct NamedContext<Name> {
    /// The sub-context made visible to the inner API.
    pub context: Context,
    _marker: PhantomData<fn() -> Name>,
}

impl<Name> NamedContext<Name> {
    /// Wrap a sub-context under the marker `Name`.
    pub fn new(context: Context) -> Self {
        NamedContext {
            context,
            _marker: PhantomData,
        }
    }
}
