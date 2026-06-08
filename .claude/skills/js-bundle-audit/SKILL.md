# JS Bundle Audit

Use this skill when auditing JavaScript bundles, generated docs assets, generated clients, or copied frontend assets in `servant-rs`.

Do not use this for normal Rust source review.

## Inputs

Ask for or infer:

- Bundle paths, source map paths, or generated docs asset directories.
- The build command that produced the assets.
- Whether the audit is security-focused, size-focused, behavior-focused, or license-focused.

## Workflow

1. Inventory assets with `rg --files` and file sizes.
2. Identify generated, vendored, minified, and hand-written files.
3. Search for secrets and local-only values:
   - API keys, bearer tokens, cookies, JWTs, passwords, private URLs, localhost-only endpoints, and source-map leaks.
4. Search for dangerous browser and Node APIs:
   - `eval`, `new Function`, dynamic script injection, `document.write`, unsafe `innerHTML`, credentialed fetches, filesystem/process access in bundled Node code.
5. Check network behavior:
   - Hard-coded origins, unexpected third-party calls, insecure `http://` URLs, missing credential policy, and CORS assumptions.
6. Check dependency and license clues:
   - Package metadata, embedded license banners, bundled dependency names, source maps, and generated client code.
7. Check size and performance:
   - Large duplicated libraries, source maps shipped unintentionally, debug builds, uncompressed assets, and avoidable polyfills.
8. If source maps exist, inspect original source paths and confirm they do not expose private filesystem details or secrets.

## Output

Lead with findings ordered by severity. Include paths and exact evidence. Then list the commands run and any files that could not be inspected.

Do not modify bundles unless the user asks for fixes.
