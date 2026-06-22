---
name: add-new-combinator-or-endpoint
description: Workflow command scaffold for add-new-combinator-or-endpoint in servant-rs.
allowed_tools: ["Bash", "Read", "Write", "Grep", "Glob"]
---

# /add-new-combinator-or-endpoint

Use this workflow when working on **add-new-combinator-or-endpoint** in `servant-rs`.

## Goal

Implements a new API combinator or endpoint across the servant ecosystem, including client, server, documentation, and OpenAPI support, with accompanying tests.

## Common Files

- `servant/src/api/args.rs`
- `servant/src/api/combinators.rs`
- `servant/src/api/constructors.rs`
- `servant/src/api/endpoint.rs`
- `servant/src/api.rs`
- `servant/src/lib.rs`

## Suggested Sequence

1. Understand the current state and failure mode before editing.
2. Make the smallest coherent change that satisfies the workflow goal.
3. Run the most relevant verification for touched files.
4. Summarize what changed and what still needs review.

## Typical Commit Signals

- Update or add implementation files in servant/src/api/* and servant/src/lib.rs
- Update or add related files in servant-client/src/client/*, servant-client/src/request.rs, and servant-client/src/lib.rs
- Update or add related files in servant-server/src/extract/*, servant-server/src/serve.rs, servant-server/src/handler.rs, and servant-server/src/lib.rs
- Update or add documentation support in servant-docs/src/* and servant-docs/src/walk/*
- Update or add OpenAPI support in servant-openapi/src/*

## Notes

- Treat this as a scaffold, not a hard-coded script.
- Update the command if the workflow evolves materially.