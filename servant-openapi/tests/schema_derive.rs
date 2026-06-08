//! `#[derive(ToSchema)]`: an OpenAPI object schema from a struct's fields.

use servant_openapi::ToSchema;

#[derive(ToSchema)]
#[allow(dead_code)]
struct User {
    id: u64,
    name: String,
    active: bool,
    nickname: Option<String>,
    tags: Vec<String>,
}

#[test]
fn derives_object_schema_with_typed_properties() {
    let schema = User::schema();
    assert_eq!(schema["type"], "object");

    let props = &schema["properties"];
    assert_eq!(props["id"]["type"], "integer");
    assert_eq!(props["name"]["type"], "string");
    assert_eq!(props["active"]["type"], "boolean");
    // Option<String> -> the inner string schema.
    assert_eq!(props["nickname"]["type"], "string");
    assert_eq!(props["tags"]["type"], "array");

    // Non-Option fields are required; the Option is not.
    let required: Vec<&str> = schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(required.contains(&"id"));
    assert!(required.contains(&"name"));
    assert!(required.contains(&"active"));
    assert!(required.contains(&"tags"));
    assert!(!required.contains(&"nickname"));
}
