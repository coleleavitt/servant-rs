---
name: extend-openapi-or-docs-metadata
description: Workflow command scaffold for extend-openapi-or-docs-metadata in servant-rs.
allowed_tools: ["Bash", "Read", "Write", "Grep", "Glob"]
---

# /extend-openapi-or-docs-metadata

Use this workflow when working on **extend-openapi-or-docs-metadata** in `servant-rs`.

## Goal

Enhances or adds richer metadata to OpenAPI schemas and documentation, propagating changes through docs, OpenAPI, and example/test files.

## Common Files

- `servant-docs/src/model.rs`
- `servant-docs/src/schema.rs`
- `servant-docs/src/walk.rs`
- `servant-openapi/src/openapi.rs`
- `servant-openapi/src/schema.rs`
- `servant-openapi/src/walk.rs`

## Suggested Sequence

1. Understand the current state and failure mode before editing.
2. Make the smallest coherent change that satisfies the workflow goal.
3. Run the most relevant verification for touched files.
4. Summarize what changed and what still needs review.

## Typical Commit Signals

- Update servant-docs/src/model.rs, servant-docs/src/schema.rs, servant-docs/src/walk.rs, and related files
- Update servant-openapi/src/openapi.rs, servant-openapi/src/schema.rs, servant-openapi/src/walk/*, and related files
- Update or add relevant tests in servant-openapi/tests/*
- Update example or reference files in servant-server/examples/* and servant-server/tests/*
- Update docs/DESIGN.md or other documentation as needed

## Notes

- Treat this as a scaffold, not a hard-coded script.
- Update the command if the workflow evolves materially.