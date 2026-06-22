```markdown
# servant-rs Development Patterns

> Auto-generated skill from repository analysis

## Overview

This skill teaches you how to contribute to the `servant-rs` Rust framework, which provides a type-safe, composable API toolkit inspired by Haskell's Servant. You'll learn the repository's coding conventions, commit patterns, and the main development workflows for adding new API combinators/endpoints, extending OpenAPI/docs metadata, and maintaining comprehensive test harnesses.

## Coding Conventions

### File Naming

- Use **snake_case** for all file and module names.

  **Example:**
  ```
  servant/src/api/combinators.rs
  servant-server/tests/comprehensive.rs
  ```

### Import Style

- Use **relative imports** within the crate.

  **Example:**
  ```rust
  use super::extractors::SomeExtractor;
  use crate::api::endpoint::Endpoint;
  ```

### Export Style

- Use **named exports** (i.e., `pub fn`, `pub struct`, `pub mod`).

  **Example:**
  ```rust
  pub mod combinators;
  pub struct Query<T> { ... }
  pub fn serve() { ... }
  ```

### Commit Messages

- Follow **conventional commit** style.
- Prefixes: `feat`, `docs`, `fix`, `test`
- Keep messages concise (average ~35 characters).

  **Example:**
  ```
  feat: add deep query combinator
  fix: correct host parsing logic
  docs: update OpenAPI schema docs
  test: add parity tests for endpoints
  ```

## Workflows

### Add New Combinator or Endpoint

**Trigger:** When you want to add a new API combinator or endpoint (e.g., query string, deep query, host, raw, stream body).

**Command:** `/new-combinator`

1. Update or add implementation files in `servant/src/api/*` and `servant/src/lib.rs`.
2. Update or add related files in:
   - `servant-client/src/client/*`
   - `servant-client/src/request.rs`
   - `servant-client/src/lib.rs`
3. Update or add related files in:
   - `servant-server/src/extract/*`
   - `servant-server/src/serve.rs`
   - `servant-server/src/handler.rs`
   - `servant-server/src/lib.rs`
4. Update or add documentation support in `servant-docs/src/*` and `servant-docs/src/walk/*`.
5. Update or add OpenAPI support in `servant-openapi/src/*`.
6. Add or update tests in:
   - `servant-client/tests/*`
   - `servant-server/tests/*`
   - `servant-docs/tests/*`
   - `servant-openapi/tests/*`
7. Add new module files if needed (e.g., `servant/src/host.rs`, `servant/src/query/deep.rs`).

**Example:**
```rust
// servant/src/api/combinators.rs
pub struct DeepQuery<T> { ... }
```

### Extend OpenAPI or Docs Metadata

**Trigger:** When you want to improve the metadata or schema information exposed by the framework's documentation or OpenAPI output.

**Command:** `/improve-openapi-metadata`

1. Update files such as:
   - `servant-docs/src/model.rs`
   - `servant-docs/src/schema.rs`
   - `servant-docs/src/walk.rs`
2. Update OpenAPI files:
   - `servant-openapi/src/openapi.rs`
   - `servant-openapi/src/schema.rs`
   - `servant-openapi/src/walk/*`
3. Update or add relevant tests in `servant-openapi/tests/*`.
4. Update example or reference files in:
   - `servant-server/examples/*`
   - `servant-server/tests/*`
5. Update documentation as needed (e.g., `docs/DESIGN.md`).

**Example:**
```rust
// servant-openapi/src/schema.rs
pub fn add_description(schema: &mut Schema, desc: &str) { ... }
```

### Add or Update Test Harness

**Trigger:** When you want to ensure new features or compatibility via comprehensive or parity test suites.

**Command:** `/add-test-harness`

1. Add or update test harness files in `servant-server/tests/comprehensive.rs` or similar.
2. Add or update support files in `servant-server/tests/support/*`.
3. Add or update UI or regression test files in `servant-server/tests/ui/*`.
4. Add or update test files in:
   - `servant-client/tests/*`
   - `servant-docs/tests/*`
   - `servant-openapi/tests/*`

**Example:**
```rust
// servant-server/tests/comprehensive.rs
#[test]
fn test_all_endpoints() {
    // Comprehensive test logic
}
```

## Testing Patterns

- Test files are typically placed in `tests/` directories within each crate.
- Test files follow the pattern: `*.rs` (Rust integration tests).
- Some references to `*.test.ts` suggest possible TypeScript/JS interop or documentation, but the primary test framework is Rust's built-in test harness.
- Use `#[test]` attribute for test functions.

**Example:**
```rust
// servant-client/tests/query.rs
#[test]
fn test_query_param() {
    // Test implementation
}
```

## Commands

| Command                | Purpose                                                            |
|------------------------|--------------------------------------------------------------------|
| /new-combinator        | Add a new API combinator or endpoint across the ecosystem          |
| /improve-openapi-metadata | Enhance OpenAPI schemas and documentation metadata               |
| /add-test-harness      | Add or expand comprehensive/parity test suites                     |
```
