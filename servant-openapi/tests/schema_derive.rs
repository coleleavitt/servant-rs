//! `#[derive(ToSchema)]`: an OpenAPI object schema from a struct's fields.

use servant_openapi::ToSchema;

#[derive(ToSchema)]
#[allow(dead_code)]
struct Address {
    street: String,
    zip: u64,
}

#[derive(ToSchema)]
#[allow(dead_code)]
struct User {
    id: u64,
    name: String,
    active: bool,
    nickname: Option<String>,
    tags: Vec<String>,
    aliases: [String; 2],
    address: Address,
    previous_addresses: Vec<Address>,
    previous_address: Option<Address>,
    optional_tags: Option<Vec<String>>,
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
    assert_eq!(props["tags"]["items"]["type"], "string");
    assert_eq!(props["aliases"]["type"], "array");
    assert_eq!(props["aliases"]["items"]["type"], "string");
    assert_eq!(props["aliases"]["minItems"], 2);
    assert_eq!(props["aliases"]["maxItems"], 2);
    assert_eq!(props["address"]["type"], "object");
    assert_eq!(props["address"]["properties"]["street"]["type"], "string");
    assert_eq!(props["address"]["properties"]["zip"]["type"], "integer");
    assert_eq!(props["previous_addresses"]["type"], "array");
    assert_eq!(props["previous_addresses"]["items"]["type"], "object");
    assert_eq!(
        props["previous_addresses"]["items"]["properties"]["street"]["type"],
        "string"
    );
    assert_eq!(props["previous_address"]["type"], "object");
    assert_eq!(
        props["previous_address"]["properties"]["street"]["type"],
        "string"
    );
    assert_eq!(props["optional_tags"]["type"], "array");
    assert_eq!(props["optional_tags"]["items"]["type"], "string");

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
    assert!(required.contains(&"aliases"));
    assert!(required.contains(&"address"));
    assert!(required.contains(&"previous_addresses"));
    assert!(!required.contains(&"nickname"));
    assert!(!required.contains(&"previous_address"));
    assert!(!required.contains(&"optional_tags"));
}
