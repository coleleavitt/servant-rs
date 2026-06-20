use serde::{Deserialize, Serialize};
use servant::prelude::*;
use servant_openapi::{OpenApiInfo, openapi_for};

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
