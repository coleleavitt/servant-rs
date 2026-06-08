//! HTTP method markers used as the `M` parameter of [`crate::api::Verb`].
//!
//! Method is carried at the type level (so `Get<…>` and `Post<…>` are distinct
//! API types) and reflected to an [`http::Method`] at runtime.

/// A type-level HTTP method.
pub trait MethodMarker {
    /// The runtime method this marker denotes.
    fn method() -> http::Method;
}

macro_rules! methods {
    ( $( $name:ident => $method:ident ),* $(,)? ) => {
        $(
            #[doc = concat!("The `", stringify!($method), "` method marker.")]
            #[derive(Debug, Clone, Copy, Default)]
            pub struct $name;
            impl MethodMarker for $name {
                fn method() -> http::Method {
                    http::Method::$method
                }
            }
        )*
    };
}

methods! {
    Get => GET,
    Post => POST,
    Put => PUT,
    Delete => DELETE,
    Patch => PATCH,
    Head => HEAD,
    Options => OPTIONS,
    Trace => TRACE,
    Connect => CONNECT,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_map_to_methods() {
        assert_eq!(Get::method(), http::Method::GET);
        assert_eq!(Delete::method(), http::Method::DELETE);
        assert_eq!(Patch::method(), http::Method::PATCH);
    }
}
