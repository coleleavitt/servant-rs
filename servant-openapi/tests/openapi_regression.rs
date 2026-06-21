use serde::{Deserialize, Serialize};
use servant::prelude::*;
use servant_openapi::{OpenApiError, OpenApiInfo, checked_openapi_for, openapi_for};

#[derive(Serialize, Deserialize)]
struct NewItem {
    name: String,
}

#[derive(Serialize, Deserialize)]
struct Item {
    id: u64,
    name: String,
}

#[test]
fn exact_openapi_output_for_metadata_paths_body_and_media_types() {
    let api = summary(
        "Create item",
        description(
            "Creates an item from a JSON request body.",
            path(
                "items",
                req_body::<(Json,), NewItem, _>(verb::<Post, 201, (Json, PlainText), Item>()),
            ),
        ),
    );
    let doc = openapi_for(
        &api,
        OpenApiInfo::new("Regression API", "2026.06")
            .with_description("Exact generated-shape regression coverage."),
    );

    assert_eq!(
        doc,
        serde_json::json!({
            "openapi": "3.0.3",
            "info": {
                "title": "Regression API",
                "version": "2026.06",
                "description": "Exact generated-shape regression coverage."
            },
            "paths": {
                "/items": {
                    "post": {
                        "summary": "Create item",
                        "description": "Creates an item from a JSON request body.",
                        "requestBody": {
                            "required": true,
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "title": "NewItem"
                                    }
                                }
                            }
                        },
                        "responses": {
                            "201": {
                                "description": "",
                                "content": {
                                    "application/json": { "schema": {} },
                                    "text/plain": { "schema": {} }
                                }
                            }
                        }
                    }
                }
            }
        })
    );
}

#[test]
fn operation_id_emits_openapi_operation_id() {
    let api = operation_id(
        "createItem",
        path("items", verb::<Post, 201, (Json,), Item>()),
    );
    let doc = openapi_for(&api, OpenApiInfo::new("Operations", "1.0.0"));

    assert_eq!(doc["paths"]["/items"]["post"]["operationId"], "createItem");
}

#[test]
fn operation_id_duplicate_returns_checked_error() {
    let api = alt(
        operation_id("duplicateId", path("left", get::<(Json,), Item>())),
        operation_id("duplicateId", path("right", get::<(Json,), Item>())),
    );

    let err = checked_openapi_for(&api, OpenApiInfo::new("Operations", "1.0.0"))
        .expect_err("duplicate operationId should be rejected");
    assert_eq!(
        err,
        OpenApiError::DuplicateOperationId {
            operation_id: "duplicateId".to_string()
        }
    );

    let compatible = openapi_for(&api, OpenApiInfo::new("Operations", "1.0.0"));
    assert_eq!(
        compatible["paths"]["/left"]["get"]["operationId"],
        "duplicateId"
    );
    assert_eq!(
        compatible["paths"]["/right"]["get"]["operationId"],
        "duplicateId"
    );
}
