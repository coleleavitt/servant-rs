# Release checklist

`servant-rs` is currently targeting an initial `0.1.0` release. The `0.x` line
does not promise semver-stable public APIs yet, but every release should still
be reproducible and documented.

## Version policy

- Use `0.1.x` patch releases for bug fixes, documentation fixes, and additive
  tests that do not change public API behavior.
- Use `0.y.0` minor releases for public API additions, ergonomic helper APIs,
  new combinators, or intentional breaking changes while the project is pre-1.0.
- Record user-visible changes in `CHANGELOG.md` before publishing.

## Pre-publish checks

Run the same checks as CI:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --all-features
```

Then dry-run every crate intended for publication:

```sh
cargo publish -p servant --dry-run
cargo publish -p servant-server --dry-run
cargo publish -p servant-client --dry-run
cargo publish -p servant-docs --dry-run
cargo publish -p servant-openapi --dry-run
cargo publish -p servant-macros --dry-run
```

Publish dependency crates first (`servant-macros`, `servant`, then the
interpretation crates), allowing crates.io indexing time between steps.
