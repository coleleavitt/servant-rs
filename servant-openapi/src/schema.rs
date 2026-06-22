//! Compatibility re-exports for schema metadata used by OpenAPI generation.

pub use servant_docs::{SchemaDoc, ToSchema, schema_for, short_name};

#[cfg(test)]
mod tests {
    use serde_json::json;

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

    #[test]
    fn collection_to_schema_is_structural() {
        assert_eq!(
            <Vec<u64> as ToSchema>::schema(),
            json!({
                "type": "array",
                "items": { "type": "integer" }
            })
        );
        assert_eq!(
            <Option<String> as ToSchema>::schema(),
            json!({
                "type": "string",
                "nullable": true
            })
        );
    }
}
