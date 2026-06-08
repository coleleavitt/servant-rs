//! Mapping from Rust `type_name`s (as carried by the shared
//! [`servant_docs`](servant_docs) model) to OpenAPI schema objects.
//!
//! **[diff]** Haskell servant-swagger derives a full JSON Schema for every type
//! via `ToSchema`. The servant-rs documentation model only carries the Rust
//! [`std::any::type_name`] string for captures, query parameters, bodies, and
//! responses — not a structural schema. So this module performs a deliberately
//! small, name-based mapping: primitives map to their OpenAPI primitive schema,
//! anything looking like a sequence maps to an untyped array, and everything
//! else maps to a generic `object` carrying the short type name as its `title`.
//!
//! This is intentionally a placeholder for a future derive-based schema layer
//! (see [`ToSchema`]); it keeps the generated document valid and useful without
//! requiring every documented type to opt in to schema reflection.

use serde_json::{Value, json};

/// A type that can describe itself as an OpenAPI schema object.
///
/// **[diff]** This mirrors the role of Haskell servant-swagger's `ToSchema`,
/// but the servant-rs docs model does not yet thread `ToSchema` bounds through
/// the combinator chain — the generator works purely from `type_name` strings
/// (see [`schema_for`]). The trait is provided so primitives have a single
/// source of truth and so a future derive can extend the mapping without
/// changing the generator. Most callers should prefer [`schema_for`].
pub trait ToSchema {
    /// The OpenAPI schema object describing `Self`.
    fn schema() -> Value;
}

macro_rules! impl_to_schema {
    ($ty:ty => $body:expr) => {
        impl ToSchema for $ty {
            fn schema() -> Value {
                $body
            }
        }
    };
}

impl_to_schema!(u8 => json!({ "type": "integer" }));
impl_to_schema!(u16 => json!({ "type": "integer" }));
impl_to_schema!(u32 => json!({ "type": "integer" }));
impl_to_schema!(u64 => json!({ "type": "integer" }));
impl_to_schema!(usize => json!({ "type": "integer" }));
impl_to_schema!(i8 => json!({ "type": "integer" }));
impl_to_schema!(i16 => json!({ "type": "integer" }));
impl_to_schema!(i32 => json!({ "type": "integer" }));
impl_to_schema!(i64 => json!({ "type": "integer" }));
impl_to_schema!(isize => json!({ "type": "integer" }));
impl_to_schema!(f32 => json!({ "type": "number" }));
impl_to_schema!(f64 => json!({ "type": "number" }));
impl_to_schema!(bool => json!({ "type": "boolean" }));
impl_to_schema!(String => json!({ "type": "string" }));
impl_to_schema!(&str => json!({ "type": "string" }));

/// Strip a fully-qualified Rust path (`alloc::string::String`,
/// `my_crate::model::User`) down to its last `::` segment, dropping any generic
/// arguments. Used for human-readable schema `title`s.
pub(crate) fn short_name(type_name: &str) -> &str {
    // Drop any generic parameters: `Foo<Bar>` -> `Foo`.
    let head = type_name.split('<').next().unwrap_or(type_name);
    // Keep only the final path segment.
    head.rsplit("::").next().unwrap_or(head).trim()
}

/// Map a Rust `type_name` (from [`std::any::type_name`]) to an OpenAPI schema.
///
/// The mapping is intentionally small (see the module docs for the `[diff]`
/// rationale):
///
/// - integers (`u8..=u64`, `i8..=i64`, `usize`, `isize`) → `{"type":"integer"}`
/// - `f32`/`f64` → `{"type":"number"}`
/// - `bool` → `{"type":"boolean"}`
/// - `String`/`&str` (and their fully-qualified spellings) → `{"type":"string"}`
/// - anything that is a `Vec`/slice → `{"type":"array","items":{}}`
/// - anything else → `{"type":"object","title":<short type name>}`
pub fn schema_for(type_name: &str) -> Value {
    let trimmed = type_name.trim();
    let short = short_name(trimmed);

    match short {
        "u8" | "u16" | "u32" | "u64" | "u128" | "usize" | "i8" | "i16" | "i32" | "i64" | "i128"
        | "isize" => json!({ "type": "integer" }),
        "f32" | "f64" => json!({ "type": "number" }),
        "bool" => json!({ "type": "boolean" }),
        "String" | "str" | "&str" => json!({ "type": "string" }),
        _ => {
            // Sequence-like types: `alloc::vec::Vec<..>`, `&[..]`, `[..]`.
            if trimmed.starts_with("alloc::vec::Vec")
                || trimmed.starts_with("Vec")
                || trimmed.starts_with('[')
                || trimmed.starts_with("&[")
                || trimmed.starts_with("[&")
            {
                json!({ "type": "array", "items": {} })
            } else {
                json!({ "type": "object", "title": short })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitive_mappings() {
        assert_eq!(schema_for("u64"), json!({ "type": "integer" }));
        assert_eq!(schema_for("i32"), json!({ "type": "integer" }));
        assert_eq!(schema_for("usize"), json!({ "type": "integer" }));
        assert_eq!(schema_for("f64"), json!({ "type": "number" }));
        assert_eq!(schema_for("bool"), json!({ "type": "boolean" }));
    }

    #[test]
    fn string_mappings() {
        assert_eq!(schema_for("String"), json!({ "type": "string" }));
        assert_eq!(
            schema_for("alloc::string::String"),
            json!({ "type": "string" })
        );
        assert_eq!(schema_for("&str"), json!({ "type": "string" }));
    }

    #[test]
    fn sequence_mappings() {
        assert_eq!(
            schema_for("alloc::vec::Vec<alloc::string::String>"),
            json!({ "type": "array", "items": {} })
        );
        assert_eq!(
            schema_for("[u8; 4]"),
            json!({ "type": "array", "items": {} })
        );
    }

    #[test]
    fn object_mapping_uses_short_title() {
        assert_eq!(
            schema_for("my_crate::model::User"),
            json!({ "type": "object", "title": "User" })
        );
        assert_eq!(
            schema_for("servant_openapi::tests::NewItem"),
            json!({ "type": "object", "title": "NewItem" })
        );
    }

    #[test]
    fn short_name_drops_generics_and_paths() {
        assert_eq!(short_name("alloc::string::String"), "String");
        assert_eq!(short_name("foo::Bar<baz::Qux>"), "Bar");
        assert_eq!(short_name("Plain"), "Plain");
    }

    #[test]
    fn to_schema_trait_matches_schema_for() {
        assert_eq!(<u64 as ToSchema>::schema(), schema_for("u64"));
        assert_eq!(<String as ToSchema>::schema(), schema_for("String"));
        assert_eq!(<bool as ToSchema>::schema(), schema_for("bool"));
    }
}
