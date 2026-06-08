# Audit servant-rs

Use this command to audit the Rust rewrite against the Haskell Servant reference.

User scope: `$ARGUMENTS`

## Procedure

1. Identify the Rust modules relevant to the requested scope.
2. Identify the matching Haskell reference files under `research/servant/`.
3. Compare behavior, not syntax.
4. Check routing, extraction, content negotiation, response rendering, client generation, auth, streaming, docs, and tests as applicable.
5. Look for security issues, missing error cases, panics in library code, unbounded buffering, and unstable public API choices.
6. Run the narrowest relevant tests, then broader workspace tests when feasible.

## Output

Lead with findings ordered by severity. Include file and line references. If no issues are found, say so and list any remaining test gaps or unverified assumptions.

Do not make edits unless the user explicitly asks for fixes.
