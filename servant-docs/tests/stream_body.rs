use servant::prelude::*;
use servant_docs::{HasDocs, markdown};

#[test]
fn stream_body_metadata_is_recorded_in_docs_model_and_markdown() {
    let api = path(
        "sum",
        stream_body::<NetstringFraming, Json, u64, _>(verb::<Post, 200, (PlainText,), String>()),
    );
    let doc = api.docs();
    let body = doc.endpoints()[0]
        .request_body
        .as_ref()
        .expect("request body is documented");

    assert_eq!(body.content_types, vec![mime::APPLICATION_JSON]);
    assert!(body.type_name.ends_with("u64"));
    assert!(body.streaming);

    let md = markdown(&doc);
    assert!(
        md.contains("Decoded incrementally as a streaming request body"),
        "md was:\n{md}"
    );
}
