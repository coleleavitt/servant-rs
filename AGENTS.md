# servant-rs Agent Instructions

This file exists for compatibility with agents that read `AGENTS.md`.

For the primary project instructions, read `CLAUDE.md` first. The goal is to rewrite Haskell Servant into idiomatic Rust in this repository.

## Project Goal

- Build `servant-rs`, a Rust rewrite inspired by Haskell Servant.
- Preserve Servant's core guarantees in Rust terms: typed API descriptions, composable routing, request extraction, content negotiation, generated clients, documentation/openapi support, auth, streaming, and strong tests.
- Do not mechanically transliterate Haskell type-level machinery when an idiomatic Rust design gives the same user-facing safety.
- Do not reduce this project to docs-only OpenAPI generation. One typed API description should drive server routing, handler shape, clients, links, and docs where practical.
- Avoid god objects. Keep API descriptions, routing, extraction, codecs, clients, docs, auth, and runtime adapters in focused modules.

## Reference Material

- `research/servant/` contains the Haskell Servant reference checkout.
- `research/servant/` is ignored by git and should be treated as read-only research input.
- Start with:
  - `research/servant/cabal.project`
  - `research/servant/servant/src/Servant/API.hs`
  - `research/servant/servant/src/Servant/Test/ComprehensiveAPI.hs`
  - `research/servant/servant-server/src/Servant/Server.hs`
  - `research/servant/servant-server/src/Servant/Server/Internal/Router.hs`
  - `research/servant/servant-client-core/src/Servant/Client/Core/HasClient.hs`

## Working Rules

- Do not edit `research/servant/` unless explicitly asked to refresh the reference checkout.
- Before implementing behavior, identify the matching Haskell reference file.
- Add tests for routing, extraction, content negotiation, client generation, docs/openapi output, auth, and streaming behavior as those features are implemented.
- Add integration tests that combine features; isolated feature examples are not enough.
- Keep Rust APIs idiomatic, documented, and hard to misuse.
- Prefer structured parsing and typed errors over stringly typed internals.

## Verification

When Rust source exists, run:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
```

If these commands cannot run because the workspace is not scaffolded yet, report that directly.
