use servant::prelude::*;
use servant_docs::{HasDocs, markdown};

#[test]
fn raw_endpoint_is_marked_opaque_in_markdown() {
    // Given: Raw and RawM endpoints under routed prefixes.
    let api = alt(
        path("api", path("files", raw())),
        path("admin", path("rawm", raw_m())),
    );

    // When: Markdown documentation is rendered from the shared docs model.
    let md = markdown(&api.docs());

    // Then: both endpoints are visible but marked opaque.
    assert!(md.contains("## OPAQUE /api/files"), "md:\n{md}");
    assert!(md.contains("## OPAQUE /admin/rawm"), "md:\n{md}");
    assert!(md.contains("`Raw` is opaque"), "md:\n{md}");
    assert!(md.contains("`RawM` is opaque"), "md:\n{md}");
}
