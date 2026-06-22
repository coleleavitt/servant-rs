//! Schema metadata shared by documentation and OpenAPI generation.

use serde_json::{Value, json};

/// Schema metadata recorded in the shared docs model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaDoc {
    /// The Rust type name, retained for Markdown and fallback schema lowering.
    pub type_name: &'static str,
    /// The OpenAPI component key to register, when structural metadata exists.
    pub component_name: Option<String>,
    /// The structural OpenAPI schema, when a type supplied one.
    pub schema: Option<Value>,
}

impl SchemaDoc {
    /// Construct a type-name-only schema record.
    pub fn type_name(type_name: &'static str) -> Self {
        SchemaDoc {
            type_name,
            component_name: None,
            schema: None,
        }
    }

    /// Construct a schema record from a [`ToSchema`] type.
    pub fn for_type<T: ToSchema>() -> Self {
        SchemaDoc {
            type_name: std::any::type_name::<T>(),
            component_name: T::component_name(),
            schema: Some(T::schema()),
        }
    }
}

/// A type that can describe itself as an OpenAPI schema object.
pub trait ToSchema {
    /// The OpenAPI schema object describing `Self`.
    fn schema() -> Value;

    /// The component key used when `Self` appears as a top-level body schema.
    fn component_name() -> Option<String> {
        Some(short_name(std::any::type_name::<Self>()).to_string())
    }
}

macro_rules! impl_primitive_schema {
    ($($ty:ty => $body:expr),* $(,)?) => {
        $(
            impl ToSchema for $ty {
                fn schema() -> Value {
                    $body
                }

                fn component_name() -> Option<String> {
                    None
                }
            }
        )*
    };
}

impl_primitive_schema! {
    u8 => json!({ "type": "integer" }),
    u16 => json!({ "type": "integer" }),
    u32 => json!({ "type": "integer" }),
    u64 => json!({ "type": "integer" }),
    u128 => json!({ "type": "integer" }),
    usize => json!({ "type": "integer" }),
    i8 => json!({ "type": "integer" }),
    i16 => json!({ "type": "integer" }),
    i32 => json!({ "type": "integer" }),
    i64 => json!({ "type": "integer" }),
    i128 => json!({ "type": "integer" }),
    isize => json!({ "type": "integer" }),
    f32 => json!({ "type": "number" }),
    f64 => json!({ "type": "number" }),
    bool => json!({ "type": "boolean" }),
    String => json!({ "type": "string" }),
    &str => json!({ "type": "string" }),
    servant::content::NoContent => json!({}),
}

impl<T: ToSchema> ToSchema for Option<T> {
    fn schema() -> Value {
        let mut schema = T::schema();
        if let Some(obj) = schema.as_object_mut() {
            obj.insert("nullable".to_string(), json!(true));
            schema
        } else {
            json!({ "nullable": true, "allOf": [schema] })
        }
    }

    fn component_name() -> Option<String> {
        None
    }
}

impl<T: ToSchema> ToSchema for Vec<T> {
    fn schema() -> Value {
        json!({ "type": "array", "items": T::schema() })
    }

    fn component_name() -> Option<String> {
        None
    }
}

impl<T: ToSchema, const N: usize> ToSchema for [T; N] {
    fn schema() -> Value {
        json!({ "type": "array", "items": T::schema(), "minItems": N, "maxItems": N })
    }

    fn component_name() -> Option<String> {
        None
    }
}

/// Strip a fully-qualified Rust path down to its last segment.
pub fn short_name(type_name: &str) -> &str {
    let head = type_name.split('<').next().unwrap_or(type_name);
    head.rsplit("::").next().unwrap_or(head).trim()
}

/// Map a Rust `type_name` to an OpenAPI schema fallback.
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
